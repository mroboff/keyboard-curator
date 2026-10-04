//! The entries offered by the key picker: every binding that can be
//! assigned with one click, grouped and searchable.

use kc_zmk::behaviors::{self, Behavior, Group, ParamKind};
use kc_zmk::keycodes::{keycodes, Category};
use kc_zmk::Feature;

use crate::behavior::BehaviorKind;
use crate::binding::{Binding, KeyExpr, Param};
use crate::project::Project;

/// The tabs of the key picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PickerGroup {
    Basic,
    Symbols,
    Modifiers,
    Navigation,
    Function,
    Keypad,
    Media,
    Apps,
    Layers,
    Mouse,
    Connectivity,
    Lighting,
    System,
    Custom,
}

impl PickerGroup {
    pub const ALL: [PickerGroup; 14] = [
        PickerGroup::Basic,
        PickerGroup::Symbols,
        PickerGroup::Modifiers,
        PickerGroup::Navigation,
        PickerGroup::Function,
        PickerGroup::Keypad,
        PickerGroup::Media,
        PickerGroup::Apps,
        PickerGroup::Layers,
        PickerGroup::Mouse,
        PickerGroup::Connectivity,
        PickerGroup::Lighting,
        PickerGroup::System,
        PickerGroup::Custom,
    ];

    pub fn title(self) -> &'static str {
        match self {
            PickerGroup::Basic => "Basic",
            PickerGroup::Symbols => "Symbols",
            PickerGroup::Modifiers => "Modifiers",
            PickerGroup::Navigation => "Navigation",
            PickerGroup::Function => "Function",
            PickerGroup::Keypad => "Keypad",
            PickerGroup::Media => "Media",
            PickerGroup::Apps => "Apps",
            PickerGroup::Layers => "Layers",
            PickerGroup::Mouse => "Mouse",
            PickerGroup::Connectivity => "Bluetooth",
            PickerGroup::Lighting => "Lighting",
            PickerGroup::System => "System",
            PickerGroup::Custom => "Custom",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PickerItem {
    pub group: PickerGroup,
    /// Short text for the button.
    pub label: String,
    /// A plain-language description of what the key will do.
    pub description: String,
    pub binding: Binding,
    /// Lower-case text that a search is matched against.
    search: String,
}

impl PickerItem {
    fn new(
        group: PickerGroup,
        label: impl Into<String>,
        description: impl Into<String>,
        binding: Binding,
        extra: &str,
    ) -> Self {
        let (label, description) = (label.into(), description.into());
        let search = format!("{label} {description} {extra}").to_lowercase();
        Self {
            group,
            label,
            description,
            binding,
            search,
        }
    }

    /// Whether every word of `query` appears somewhere in this item.
    pub fn matches(&self, query: &str) -> bool {
        query
            .to_lowercase()
            .split_whitespace()
            .all(|word| self.search.contains(word))
    }
}

fn keycode_group(category: Category) -> PickerGroup {
    match category {
        Category::Letters | Category::Numbers | Category::Editing => PickerGroup::Basic,
        Category::Symbols | Category::International => PickerGroup::Symbols,
        Category::Navigation => PickerGroup::Navigation,
        Category::Function => PickerGroup::Function,
        Category::Modifiers => PickerGroup::Modifiers,
        Category::Keypad => PickerGroup::Keypad,
        Category::Media => PickerGroup::Media,
        Category::Apps => PickerGroup::Apps,
        Category::System => PickerGroup::System,
    }
}

fn behavior_group(group: Group) -> PickerGroup {
    match group {
        Group::Keys => PickerGroup::Basic,
        Group::Layers => PickerGroup::Layers,
        Group::Mouse => PickerGroup::Mouse,
        Group::Connectivity => PickerGroup::Connectivity,
        Group::Lighting => PickerGroup::Lighting,
        Group::System => PickerGroup::System,
    }
}

fn behavior_items(
    project: &Project,
    def: &Behavior,
    features: &[Feature],
    out: &mut Vec<PickerItem>,
) {
    let group = behavior_group(def.group);
    let label = def.label;
    match def.params {
        [] => out.push(PickerItem::new(
            group,
            def.name,
            def.description,
            Binding::new(label, vec![]),
            label,
        )),
        [param] => match param.kind {
            // Plain keys come from the keycode catalogue; `kt` and `sk` are
            // variations chosen in the inspector.
            ParamKind::Keycode => {}
            ParamKind::Layer => {
                for layer in &project.layers {
                    out.push(PickerItem::new(
                        group,
                        format!("{} {}", label, layer.name),
                        format!("{}: {}", def.name, layer.name),
                        Binding::layer(label, layer.id),
                        "layer",
                    ));
                }
            }
            ParamKind::Constant(constants) => {
                for constant in constants {
                    out.push(PickerItem::new(
                        group,
                        constant.description,
                        format!("{}: {}", def.name, constant.description),
                        Binding::new(label, vec![Param::Constant(constant.name.into())]),
                        constant.name,
                    ));
                }
            }
            ParamKind::Command(commands) => {
                for command in commands.iter().filter(|c| c.available(features)) {
                    // One entry per value for a single small argument, such
                    // as the five Bluetooth profiles. Commands with several
                    // arguments are set up in the inspector.
                    let values: Vec<Vec<u32>> = match command.args {
                        [] => vec![vec![]],
                        [arg] if arg.max - arg.min < 8 => {
                            (arg.min..=arg.max).map(|v| vec![v]).collect()
                        }
                        _ => continue,
                    };
                    for args in values {
                        let suffix: String = args.iter().map(|a| format!(" {a}")).collect();
                        out.push(PickerItem::new(
                            group,
                            format!("{}{suffix}", command.name.replace('_', " ")),
                            format!("{}{suffix}", command.description),
                            Binding::new(
                                label,
                                vec![Param::Command {
                                    name: command.name.into(),
                                    args,
                                }],
                            ),
                            command.name,
                        ));
                    }
                }
            }
        },
        // Two-parameter behaviours (`mt`, `lt`) are built in the inspector.
        _ => {}
    }
}

/// Every one-click binding available to `project` on a firmware with
/// `features`, in display order.
pub fn picker_items(project: &Project, features: &[Feature]) -> Vec<PickerItem> {
    let mut items = Vec::new();
    for key in keycodes().all() {
        let aliases: Vec<&str> = key.aliases.iter().map(|a| a.name.as_str()).collect();
        items.push(PickerItem::new(
            keycode_group(key.category),
            key.legend.clone(),
            key.description.clone(),
            Binding::kp(KeyExpr::new(key.short_name())),
            &format!("{} {}", key.name, aliases.join(" ")),
        ));
    }
    for def in behaviors::BUILT_IN.iter().filter(|b| b.available(features)) {
        behavior_items(project, def, features, &mut items);
    }
    for def in &project.behaviors {
        // Behaviours that take parameters are assigned through the inspector.
        if def.kind.param_count() == 0 {
            let what = match def.kind {
                BehaviorKind::Macro(_) => "Macro",
                BehaviorKind::TapDance(_) => "Tap-dance",
                BehaviorKind::ModMorph(_) => "Mod-morph",
                _ => "Behaviour",
            };
            items.push(PickerItem::new(
                PickerGroup::Custom,
                def.name.clone(),
                format!("{what}: {}", def.name),
                Binding::user(def.id, vec![]),
                &def.label,
            ));
        }
    }
    items
}
