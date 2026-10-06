//! Small transformations of a binding, as the key inspector and picker
//! apply them: changing the tapped key, toggling a modifier, adding a hold.

use kc_zmk::Modifier;

use crate::binding::{BehaviorRef, Binding, KeyExpr, Param};
use crate::ids::LayerId;

/// What holding a key does, alongside what tapping it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    None,
    Modifier(Modifier),
    Layer(LayerId),
}

fn built_in(binding: &Binding) -> Option<(&str, &[Param])> {
    match binding {
        Binding::Behavior {
            behavior: BehaviorRef::BuiltIn(label),
            params,
        } => Some((label.as_str(), params.as_slice())),
        _ => None,
    }
}

/// The key a binding sends when tapped, for `&kp`, `&mt` and `&lt`.
pub fn tap_key(binding: &Binding) -> Option<&KeyExpr> {
    match built_in(binding)? {
        ("kp", [Param::Key(key)]) => Some(key),
        ("mt" | "lt", [_, Param::Key(key)]) => Some(key),
        _ => None,
    }
}

/// What a binding does when held, for `&mt` and `&lt`.
pub fn hold(binding: &Binding) -> Hold {
    match built_in(binding) {
        Some(("mt", [Param::Key(key), _])) => {
            Modifier::from_keycode(&key.key).map_or(Hold::None, Hold::Modifier)
        }
        Some(("lt", [Param::Layer(layer), _])) => Hold::Layer(*layer),
        _ => Hold::None,
    }
}

fn build(hold: Hold, tap: KeyExpr) -> Binding {
    match hold {
        Hold::None => Binding::kp(tap),
        Hold::Modifier(modifier) => Binding::new(
            "mt",
            vec![
                Param::Key(KeyExpr::new(modifier.keycode())),
                Param::Key(tap),
            ],
        ),
        Hold::Layer(layer) => Binding::new("lt", vec![Param::Layer(layer), Param::Key(tap)]),
    }
}

/// `binding` with its hold changed. Only bindings that tap a key can hold.
pub fn with_hold(binding: &Binding, new_hold: Hold) -> Option<Binding> {
    Some(build(new_hold, tap_key(binding)?.clone()))
}

/// `binding` with `modifier` added to, or removed from, its tapped key.
pub fn toggle_modifier(binding: &Binding, modifier: Modifier) -> Option<Binding> {
    let mut tap = tap_key(binding)?.clone();
    match tap.mods.iter().position(|m| *m == modifier) {
        Some(index) => {
            tap.mods.remove(index);
        }
        None => tap.mods.push(modifier),
    }
    Some(build(hold(binding), tap))
}

/// The binding a key gets when `picked` is chosen in the picker. Picking a
/// plain key for a binding that taps a key and does something else as well
/// (a mod-tap, layer-tap or user-defined hold-tap) replaces only what a tap
/// sends; anything else replaces the binding outright.
pub fn assign(current: &Binding, picked: &Binding) -> Binding {
    let Some(("kp", [Param::Key(key)])) = built_in(picked) else {
        return picked.clone();
    };
    let mut updated = current.clone();
    if let Binding::Behavior { params, .. } = &mut updated {
        if let [_, .., Param::Key(tap)] = params.as_mut_slice() {
            *tap = key.clone();
            return updated;
        }
    }
    picked.clone()
}

/// The numeric arguments of a command binding such as `&bt BT_SEL 2`, with
/// the catalog's description of each.
pub fn command_args(binding: &Binding) -> Vec<(&'static kc_zmk::behaviors::Arg, u32)> {
    let Some((label, [Param::Command { name, args }])) = built_in(binding) else {
        return Vec::new();
    };
    let commands = kc_zmk::behaviors::built_in(label)
        .into_iter()
        .flat_map(|def| def.params)
        .find_map(|p| match p.kind {
            kc_zmk::behaviors::ParamKind::Command(commands) => Some(commands),
            _ => None,
        });
    let Some(command) = commands.and_then(|c| c.iter().find(|c| c.name == name)) else {
        return Vec::new();
    };
    command.args.iter().zip(args.iter().copied()).collect()
}

/// `binding` with one command argument changed, kept within its range.
pub fn with_command_arg(binding: &Binding, index: usize, value: i64) -> Option<Binding> {
    let limits = command_args(binding);
    let (arg, _) = limits.get(index)?;
    let mut updated = binding.clone();
    if let Binding::Behavior { params, .. } = &mut updated {
        if let [Param::Command { args, .. }] = params.as_mut_slice() {
            args[index] = value.clamp(i64::from(arg.min), i64::from(arg.max)) as u32;
        }
    }
    Some(updated)
}

/// `binding` with the layer parameter at `index` pointed at another layer.
pub fn with_layer(binding: &Binding, index: usize, layer: LayerId) -> Option<Binding> {
    let mut updated = binding.clone();
    let Binding::Behavior { params, .. } = &mut updated else {
        return None;
    };
    match params.get_mut(index)? {
        Param::Layer(current) => *current = layer,
        _ => return None,
    }
    Some(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kp(key: &str) -> Binding {
        Binding::kp(key.parse().unwrap())
    }

    #[test]
    fn modifiers_toggle_on_the_tapped_key() {
        let plain = kp("K");
        let shifted = toggle_modifier(&plain, Modifier::LShift).unwrap();
        assert_eq!(shifted, kp("LS(K)"));
        let both = toggle_modifier(&shifted, Modifier::LCtrl).unwrap();
        assert_eq!(both, kp("LS(LC(K))"));
        assert_eq!(
            toggle_modifier(&both, Modifier::LShift).unwrap(),
            kp("LC(K)")
        );
        assert_eq!(toggle_modifier(&Binding::trans(), Modifier::LShift), None);
    }

    #[test]
    fn holds_convert_between_kp_mod_tap_and_layer_tap() {
        let plain = kp("LG(A)");
        let mod_tap = with_hold(&plain, Hold::Modifier(Modifier::LShift)).unwrap();
        assert_eq!(hold(&mod_tap), Hold::Modifier(Modifier::LShift));
        assert_eq!(tap_key(&mod_tap).unwrap().to_string(), "LG(A)");

        let layer_tap = with_hold(&mod_tap, Hold::Layer(LayerId(4))).unwrap();
        assert_eq!(hold(&layer_tap), Hold::Layer(LayerId(4)));
        assert_eq!(with_hold(&layer_tap, Hold::None).unwrap(), plain);
        // Modifiers on the tap survive, and toggling keeps the hold.
        let toggled = toggle_modifier(&layer_tap, Modifier::LGui).unwrap();
        assert_eq!(
            (hold(&toggled), tap_key(&toggled).unwrap().to_string()),
            (Hold::Layer(LayerId(4)), "A".into())
        );
        assert_eq!(with_hold(&Binding::none(), Hold::None), None);
    }

    #[test]
    fn mod_tap_holds_are_recognized_by_any_alias() {
        let binding = Binding::new(
            "mt",
            vec![
                Param::Key(KeyExpr::new("LEFT_SHIFT")),
                Param::Key(KeyExpr::new("A")),
            ],
        );
        assert_eq!(hold(&binding), Hold::Modifier(Modifier::LShift));
    }

    #[test]
    fn picking_a_key_keeps_an_existing_hold() {
        let mod_tap = with_hold(&kp("A"), Hold::Modifier(Modifier::LCtrl)).unwrap();
        let picked = assign(&mod_tap, &kp("B"));
        assert_eq!(
            (hold(&picked), tap_key(&picked).unwrap().key.as_str()),
            (Hold::Modifier(Modifier::LCtrl), "B")
        );
        assert_eq!(assign(&kp("A"), &kp("B")), kp("B"));
        assert_eq!(assign(&mod_tap, &Binding::trans()), Binding::trans());
        // A user-defined hold-tap keeps its hold parameter too.
        let custom = Binding::user(
            crate::ids::BehaviorId(7),
            vec![Param::Layer(LayerId(2)), Param::Key(KeyExpr::new("A"))],
        );
        assert_eq!(
            assign(&custom, &kp("ESC")),
            Binding::user(
                crate::ids::BehaviorId(7),
                vec![Param::Layer(LayerId(2)), Param::Key(KeyExpr::new("ESC"))],
            )
        );
    }

    #[test]
    fn command_arguments_are_adjusted_within_their_range() {
        let select = Binding::new(
            "bt",
            vec![Param::Command {
                name: "BT_SEL".into(),
                args: vec![2],
            }],
        );
        let args = command_args(&select);
        assert_eq!((args[0].0.name, args[0].1), ("profile", 2));
        let args_of = |b: &Binding| command_args(b).iter().map(|a| a.1).collect::<Vec<_>>();
        assert_eq!(args_of(&with_command_arg(&select, 0, 3).unwrap()), [3]);
        assert_eq!(args_of(&with_command_arg(&select, 0, 99).unwrap()), [4]);
        assert_eq!(args_of(&with_command_arg(&select, 0, -5).unwrap()), [0]);
        assert_eq!(with_command_arg(&select, 1, 0), None);
        assert!(command_args(&kp("A")).is_empty());

        let momentary = Binding::layer("mo", LayerId(1));
        assert_eq!(
            with_layer(&momentary, 0, LayerId(5)),
            Some(Binding::layer("mo", LayerId(5)))
        );
        assert_eq!(with_layer(&kp("A"), 0, LayerId(5)), None);
    }
}
