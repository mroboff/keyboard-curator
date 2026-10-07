//! The project: one keyboard's complete configuration, and the operations
//! that edit it while keeping every reference intact.

use kc_boards::Board;
use serde::{Deserialize, Serialize};

use crate::behavior::{BehaviorDef, BehaviorKind};
use crate::binding::{BehaviorRef, Binding, KeyExpr};
use crate::features::{
    Combo, ConditionalLayer, InputProcessor, KeyLight, LayerLighting, PointingConfig, RawBlocks,
    Rgb,
};
use crate::ids::{BehaviorId, ComboId, LayerId};

/// The project file format version this build writes.
pub const FORMAT: u32 = 2;

/// ZMK's limit on layers, counting those reserved for ZMK Studio.
pub const MAX_LAYERS: usize = 32;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    /// A color tag for the layer list. Not sent to the keyboard.
    pub color: Option<Rgb>,
    /// One binding per key position of the project's physical layout.
    pub bindings: Vec<Binding>,
}

/// Somewhere in a project, for reporting references and problems.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Location {
    Project,
    Key {
        layer: LayerId,
        position: usize,
    },
    Behavior(BehaviorId),
    Combo(ComboId),
    /// A conditional-layer rule, by index.
    ConditionalLayer(usize),
    /// A pointing device, by listener label.
    Pointing(String),
    Lighting(LayerId),
    Setting(String),
}

/// A binding that lives inside a behavior or combo rather than on a key,
/// so that editors can point the key picker at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    /// One tap count of a tap-dance.
    TapDance {
        behavior: BehaviorId,
        index: usize,
    },
    /// The normal or the morphed binding of a mod-morph.
    ModMorph {
        behavior: BehaviorId,
        morphed: bool,
    },
    /// One binding within a tap, press or release step of a macro.
    MacroStep {
        behavior: BehaviorId,
        step: usize,
        index: usize,
    },
    Combo(ComboId),
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ModelError {
    #[error("that binding no longer exists")]
    NoSuchSlot,
    #[error("no layer with id {0:?}")]
    NoSuchLayer(LayerId),
    #[error("no behavior with id {0:?}")]
    NoSuchBehavior(BehaviorId),
    #[error("no combo with id {0:?}")]
    NoSuchCombo(ComboId),
    #[error("key position {position} is outside the layout's {keys} keys")]
    NoSuchPosition { position: usize, keys: usize },
    #[error("a keymap can have at most {MAX_LAYERS} layers, including reserved ones")]
    TooManyLayers,
    #[error("a keymap needs at least one layer")]
    LastLayer,
    #[error("the layer is still referred to from {} place(s)", .0.len())]
    LayerInUse(Vec<Location>),
    #[error("the behavior is still referred to from {} place(s)", .0.len())]
    BehaviorInUse(Vec<Location>),
    #[error("`{0}` is not a valid behavior label: use letters, digits and underscores, not starting with a digit")]
    InvalidLabel(String),
    #[error("the label `{0}` is already taken")]
    LabelTaken(String),
    #[error("the board `{board}` has no layout `{layout}`")]
    NoSuchLayout { board: String, layout: String },
    #[error("the key map has {given} entries for {keys} keys")]
    KeyMapSize { given: usize, keys: usize },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format: u32,
    pub name: String,
    /// Board and physical layout, by their IDs in the board definition.
    /// The firmware and its settings belong to the user's board, not here.
    pub board: String,
    pub layout: String,
    /// Number of keys in the physical layout; every layer has this many
    /// bindings.
    pub key_count: usize,
    /// In keymap order: the first layer is the base layer.
    pub layers: Vec<Layer>,
    /// Empty layer slots kept for ZMK Studio to add layers into.
    pub reserved_layers: usize,
    pub behaviors: Vec<BehaviorDef>,
    pub combos: Vec<Combo>,
    pub conditional_layers: Vec<ConditionalLayer>,
    pub pointing: Vec<PointingConfig>,
    pub lighting: Vec<LayerLighting>,
    pub raw: RawBlocks,
    next_id: u32,
}

/// A layout to start a new project from: a vendor's factory layout, or a
/// community layout brought to the board with its author's credit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Template {
    /// Stable within a board; `factory` is the vendor's layout.
    pub id: &'static str,
    pub board: &'static str,
    pub name: &'static str,
    /// One or two sentences on what the layout is.
    pub description: &'static str,
    /// Whose layout it is, and where to find the original.
    pub credit: Option<Credit>,
    json: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Credit {
    pub author: &'static str,
    pub url: &'static str,
}

/// The templates that ship with the app. Factory layouts are regenerated
/// with the `make_template` example when a vendor changes theirs; the
/// TailorKey port with kc-import's `tailorkey_imprint` example.
pub const TEMPLATES: &[Template] = &[
    Template {
        id: "factory",
        board: "cyboard-imprint",
        name: "Factory layout",
        description: "Cyboard's layout as the keyboard ships: five layers with the keypad, navigation and keyboard-control layers.",
        credit: None,
        json: include_str!("../templates/cyboard-imprint.kcproj"),
    },
    Template {
        id: "tailorkey",
        board: "cyboard-imprint",
        name: "TailorKey",
        description: "Moosy Research's TailorKey v5.2 Bilateral (QWERTY) brought to the Imprint: bilateral home-row mods, autoshift, cursor, symbol, mouse and gaming layers, combos and macros, with TailorKey's per-layer colors and the trackballs standing in for the touchpads.",
        credit: Some(Credit {
            author: "Moosy Research",
            url: "https://sites.google.com/view/tailorkey",
        }),
        json: include_str!("../templates/tailorkey-cyboard-imprint.kcproj"),
    },
    Template {
        id: "factory",
        board: "moergo-go60",
        name: "Factory layout",
        description: "MoErgo's layout as the keyboard ships.",
        credit: None,
        json: include_str!("../templates/moergo-go60.kcproj"),
    },
    Template {
        id: "factory",
        board: "moergo-glove80",
        name: "Factory layout",
        description: "MoErgo's layout as the keyboard ships.",
        credit: None,
        json: include_str!("../templates/moergo-glove80.kcproj"),
    },
];

impl Project {
    /// The templates a board can start from, the factory layout first.
    pub fn templates(board: &Board) -> Vec<&'static Template> {
        TEMPLATES.iter().filter(|t| t.board == board.id).collect()
    }

    /// A new project that starts as the board's factory layout: every
    /// layer, behavior and pointing setting the keyboard ships with.
    /// Boards without a template start from [`Project::new`].
    pub fn from_template(name: impl Into<String>, board: &Board) -> Self {
        let name = name.into();
        Self::from_template_id("factory", name.clone(), board)
            .unwrap_or_else(|| Self::new(name, board))
    }

    /// A new project from one of the board's templates by id, or `None`
    /// when the board has no such template.
    pub fn from_template_id(id: &str, name: impl Into<String>, board: &Board) -> Option<Self> {
        let mut project = TEMPLATES
            .iter()
            .find(|t| t.board == board.id && t.id == id)
            .and_then(|t| crate::file::from_json(t.json).ok())
            .filter(|p| p.layout == board.default_layout)?;
        project.name = name.into();
        Some(project)
    }

    /// A new project for `board`, with a base layer of the board's starter
    /// keys.
    pub fn new(name: impl Into<String>, board: &Board) -> Self {
        let layout = board
            .layout(&board.default_layout)
            .expect("validated boards have their default layout");
        let mut project = Self {
            format: FORMAT,
            name: name.into(),
            board: board.id.clone(),
            layout: layout.id.clone(),
            key_count: layout.keys.len(),
            layers: Vec::new(),
            reserved_layers: 0,
            behaviors: Vec::new(),
            combos: Vec::new(),
            conditional_layers: Vec::new(),
            pointing: Vec::new(),
            lighting: Vec::new(),
            raw: RawBlocks::default(),
            next_id: 1,
        };
        project.push_layer("Base".into());
        for (binding, key) in project.layers[0]
            .bindings
            .iter_mut()
            .zip(&board.starter_keys)
        {
            if !key.is_empty() {
                *binding = Binding::kp(KeyExpr::new(key.clone()));
            }
        }
        project
    }

    fn fresh_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn push_layer(&mut self, name: String) -> LayerId {
        let id = LayerId(self.fresh_id());
        self.layers.push(Layer {
            id,
            name,
            color: None,
            bindings: vec![Binding::trans(); self.key_count],
        });
        id
    }

    /// Moves the project to another board's layout, key by key. `to_new`
    /// gives each old position its new one, or `None` when the key has no
    /// counterpart; keys of the new layout that nothing maps to are left
    /// transparent and unlit. Combos that lose a key are dropped and
    /// returned by name; hold-trigger positions simply lose the key.
    /// Pointing settings are kept as they are, since their listeners are
    /// the board's own to rename.
    pub fn remap_keys(
        &mut self,
        board: &Board,
        layout: &str,
        to_new: &[Option<usize>],
    ) -> Result<Vec<String>, ModelError> {
        let target = board
            .layout(layout)
            .ok_or_else(|| ModelError::NoSuchLayout {
                board: board.id.clone(),
                layout: layout.to_string(),
            })?;
        if to_new.len() != self.key_count {
            return Err(ModelError::KeyMapSize {
                given: to_new.len(),
                keys: self.key_count,
            });
        }
        let count = target.keys.len();
        if let Some(&bad) = to_new.iter().flatten().find(|&&n| n >= count) {
            return Err(ModelError::NoSuchPosition {
                position: bad,
                keys: count,
            });
        }
        let moved = |positions: &[usize]| -> Option<Vec<usize>> {
            positions
                .iter()
                .map(|&p| to_new.get(p).copied().flatten())
                .collect()
        };
        for layer in &mut self.layers {
            let mut bindings = vec![Binding::trans(); count];
            for (old, binding) in layer.bindings.iter().enumerate() {
                if let Some(new) = to_new[old] {
                    bindings[new] = binding.clone();
                }
            }
            layer.bindings = bindings;
        }
        for lighting in &mut self.lighting {
            let mut keys = vec![KeyLight::Inherit; count];
            for (old, light) in lighting.keys.iter().enumerate() {
                if let Some(new) = to_new.get(old).copied().flatten() {
                    keys[new] = *light;
                }
            }
            lighting.keys = keys;
        }
        for def in &mut self.behaviors {
            if let BehaviorKind::HoldTap(h) = &mut def.kind {
                h.hold_trigger_key_positions = h
                    .hold_trigger_key_positions
                    .iter()
                    .filter_map(|&p| to_new.get(p).copied().flatten())
                    .collect();
            }
        }
        let mut dropped = Vec::new();
        self.combos
            .retain_mut(|combo| match moved(&combo.key_positions) {
                Some(positions) => {
                    combo.key_positions = positions;
                    true
                }
                None => {
                    dropped.push(combo.name.clone());
                    false
                }
            });
        self.board = board.id.clone();
        self.layout = target.id.clone();
        self.key_count = count;
        Ok(dropped)
    }

    // Layers

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Result<&mut Layer, ModelError> {
        self.layers
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or(ModelError::NoSuchLayer(id))
    }

    /// The layer's position in the keymap, which is what ZMK calls its index.
    pub fn layer_index(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    pub fn add_layer(&mut self, name: impl Into<String>) -> Result<LayerId, ModelError> {
        if self.layers.len() + self.reserved_layers >= MAX_LAYERS {
            return Err(ModelError::TooManyLayers);
        }
        Ok(self.push_layer(name.into()))
    }

    /// Copies a layer, with its lighting, and places the copy after it.
    pub fn duplicate_layer(&mut self, id: LayerId) -> Result<LayerId, ModelError> {
        let index = self.layer_index(id).ok_or(ModelError::NoSuchLayer(id))?;
        if self.layers.len() + self.reserved_layers >= MAX_LAYERS {
            return Err(ModelError::TooManyLayers);
        }
        let new_id = LayerId(self.fresh_id());
        let mut copy = self.layers[index].clone();
        copy.id = new_id;
        copy.name = format!("{} copy", copy.name);
        self.layers.insert(index + 1, copy);
        if let Some(mut lighting) = self.lighting.iter().find(|l| l.layer == id).cloned() {
            lighting.layer = new_id;
            self.lighting.push(lighting);
        }
        Ok(new_id)
    }

    /// Moves a layer to `index`. References follow it, because they hold the
    /// layer's ID rather than its position.
    pub fn move_layer(&mut self, id: LayerId, index: usize) -> Result<(), ModelError> {
        let from = self.layer_index(id).ok_or(ModelError::NoSuchLayer(id))?;
        let layer = self.layers.remove(from);
        self.layers.insert(index.min(self.layers.len()), layer);
        Ok(())
    }

    pub fn set_reserved_layers(&mut self, count: usize) -> Result<(), ModelError> {
        if self.layers.len() + count > MAX_LAYERS {
            return Err(ModelError::TooManyLayers);
        }
        self.reserved_layers = count;
        Ok(())
    }

    /// Everywhere that refers to a layer, other than the layer's own lighting.
    pub fn layer_references(&self, id: LayerId) -> Vec<Location> {
        let mut found: Vec<Location> = self
            .bindings()
            .into_iter()
            .filter(|(_, b)| b.layers().any(|l| l == id))
            .map(|(location, _)| location)
            .collect();
        for combo in &self.combos {
            if combo.layers.contains(&id) {
                found.push(Location::Combo(combo.id));
            }
        }
        for (index, rule) in self.conditional_layers.iter().enumerate() {
            if rule.then_layer == id || rule.if_layers.contains(&id) {
                found.push(Location::ConditionalLayer(index));
            }
        }
        for device in &self.pointing {
            let in_processors = device
                .all_processors()
                .any(|p| matches!(p, InputProcessor::TempLayer { layer, .. } if *layer == id));
            if in_processors || device.overrides.iter().any(|o| o.layers.contains(&id)) {
                found.push(Location::Pointing(device.listener.clone()));
            }
        }
        found.dedup();
        found
    }

    /// Removes a layer nothing refers to.
    pub fn remove_layer(&mut self, id: LayerId) -> Result<(), ModelError> {
        let index = self.layer_index(id).ok_or(ModelError::NoSuchLayer(id))?;
        if self.layers.len() == 1 {
            return Err(ModelError::LastLayer);
        }
        let references = self.layer_references(id);
        if !references.is_empty() {
            return Err(ModelError::LayerInUse(references));
        }
        self.layers.remove(index);
        self.lighting.retain(|l| l.layer != id);
        Ok(())
    }

    /// Removes a layer and everything that depended on it: bindings to it
    /// become `&none`, and rules, combo filters and pointing settings that
    /// named it are dropped.
    pub fn remove_layer_and_references(&mut self, id: LayerId) -> Result<(), ModelError> {
        if self.layer_index(id).is_none() {
            return Err(ModelError::NoSuchLayer(id));
        }
        if self.layers.len() == 1 {
            return Err(ModelError::LastLayer);
        }
        self.for_each_binding_mut(|_, binding| {
            if binding.layers().any(|l| l == id) {
                *binding = Binding::none();
            }
        });
        // A combo limited to only this layer would otherwise become active
        // everywhere, so it goes too.
        self.combos.retain(|c| c.layers != [id]);
        for combo in &mut self.combos {
            combo.layers.retain(|l| *l != id);
        }
        self.conditional_layers
            .retain(|r| r.then_layer != id && !r.if_layers.contains(&id));
        for device in &mut self.pointing {
            let keep = |p: &InputProcessor| !matches!(p, InputProcessor::TempLayer { layer, .. } if *layer == id);
            device.processors.retain(keep);
            device.overrides.retain(|o| o.layers != [id]);
            for o in &mut device.overrides {
                o.layers.retain(|l| *l != id);
                o.processors.retain(keep);
            }
        }
        self.remove_layer(id)
    }

    pub fn set_binding(
        &mut self,
        layer: LayerId,
        position: usize,
        binding: Binding,
    ) -> Result<(), ModelError> {
        let keys = self.key_count;
        let slot = self
            .layer_mut(layer)?
            .bindings
            .get_mut(position)
            .ok_or(ModelError::NoSuchPosition { position, keys })?;
        *slot = binding;
        Ok(())
    }

    /// Exchanges the bindings of two keys on a layer.
    pub fn swap_bindings(&mut self, layer: LayerId, a: usize, b: usize) -> Result<(), ModelError> {
        let keys = self.key_count;
        let bindings = &mut self.layer_mut(layer)?.bindings;
        match [a, b].into_iter().find(|p| *p >= bindings.len()) {
            Some(position) => Err(ModelError::NoSuchPosition { position, keys }),
            None => {
                bindings.swap(a, b);
                Ok(())
            }
        }
    }

    // Lighting

    pub fn lighting(&self, layer: LayerId) -> Option<&LayerLighting> {
        self.lighting.iter().find(|l| l.layer == layer)
    }

    /// The layer's lighting, created with every key inheriting if absent.
    pub fn lighting_mut(&mut self, layer: LayerId) -> Result<&mut LayerLighting, ModelError> {
        if self.layer_index(layer).is_none() {
            return Err(ModelError::NoSuchLayer(layer));
        }
        let index = match self.lighting.iter().position(|l| l.layer == layer) {
            Some(index) => index,
            None => {
                self.lighting.push(LayerLighting {
                    layer,
                    fade_delay: None,
                    keys: vec![KeyLight::Inherit; self.key_count],
                    palette: Vec::new(),
                });
                self.lighting.len() - 1
            }
        };
        Ok(&mut self.lighting[index])
    }

    // User-defined behaviors

    pub fn behavior(&self, id: BehaviorId) -> Option<&BehaviorDef> {
        self.behaviors.iter().find(|b| b.id == id)
    }

    pub fn behavior_mut(&mut self, id: BehaviorId) -> Result<&mut BehaviorDef, ModelError> {
        self.behaviors
            .iter_mut()
            .find(|b| b.id == id)
            .ok_or(ModelError::NoSuchBehavior(id))
    }

    fn check_label(&self, label: &str, except: Option<BehaviorId>) -> Result<(), ModelError> {
        let mut chars = label.chars();
        let valid = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err(ModelError::InvalidLabel(label.to_string()));
        }
        let taken = kc_zmk::behaviors::built_in(label).is_some()
            || self
                .behaviors
                .iter()
                .any(|b| b.label == label && Some(b.id) != except);
        if taken {
            return Err(ModelError::LabelTaken(label.to_string()));
        }
        Ok(())
    }

    pub fn add_behavior(
        &mut self,
        label: impl Into<String>,
        name: impl Into<String>,
        kind: BehaviorKind,
    ) -> Result<BehaviorId, ModelError> {
        let label = label.into();
        self.check_label(&label, None)?;
        let id = BehaviorId(self.fresh_id());
        self.behaviors.push(BehaviorDef {
            id,
            label,
            name: name.into(),
            description: String::new(),
            kind,
        });
        Ok(id)
    }

    pub fn rename_behavior_label(
        &mut self,
        id: BehaviorId,
        label: impl Into<String>,
    ) -> Result<(), ModelError> {
        let label = label.into();
        self.check_label(&label, Some(id))?;
        self.behavior_mut(id)?.label = label;
        Ok(())
    }

    /// Everywhere that uses a user-defined behavior.
    pub fn behavior_references(&self, id: BehaviorId) -> Vec<Location> {
        let wanted = BehaviorRef::User { user: id };
        let mut found: Vec<Location> = self
            .bindings()
            .into_iter()
            .filter(|(_, b)| b.user_behavior() == Some(id))
            .map(|(location, _)| location)
            .collect();
        for def in &self.behaviors {
            if def.id != id && def.kind.behavior_refs().contains(&&wanted) {
                found.push(Location::Behavior(def.id));
            }
        }
        found.dedup();
        found
    }

    pub fn remove_behavior(&mut self, id: BehaviorId) -> Result<(), ModelError> {
        if self.behavior(id).is_none() {
            return Err(ModelError::NoSuchBehavior(id));
        }
        let references = self.behavior_references(id);
        if !references.is_empty() {
            return Err(ModelError::BehaviorInUse(references));
        }
        self.behaviors.retain(|b| b.id != id);
        Ok(())
    }

    // Combos

    pub fn add_combo(
        &mut self,
        name: impl Into<String>,
        key_positions: Vec<usize>,
        binding: Binding,
    ) -> ComboId {
        let id = ComboId(self.fresh_id());
        self.combos.push(Combo {
            id,
            name: name.into(),
            key_positions,
            binding,
            timeout_ms: None,
            require_prior_idle_ms: None,
            slow_release: false,
            layers: Vec::new(),
        });
        id
    }

    pub fn combo_mut(&mut self, id: ComboId) -> Result<&mut Combo, ModelError> {
        self.combos
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or(ModelError::NoSuchCombo(id))
    }

    pub fn remove_combo(&mut self, id: ComboId) -> Result<(), ModelError> {
        let before = self.combos.len();
        self.combos.retain(|c| c.id != id);
        if self.combos.len() == before {
            return Err(ModelError::NoSuchCombo(id));
        }
        Ok(())
    }

    /// The binding in a slot.
    pub fn slot(&self, slot: Slot) -> Option<&Binding> {
        use crate::behavior::MacroStep;
        match slot {
            Slot::Combo(id) => self.combos.iter().find(|c| c.id == id).map(|c| &c.binding),
            Slot::TapDance { behavior, index } => match &self.behavior(behavior)?.kind {
                BehaviorKind::TapDance(t) => t.bindings.get(index),
                _ => None,
            },
            Slot::ModMorph { behavior, morphed } => match &self.behavior(behavior)?.kind {
                BehaviorKind::ModMorph(m) => Some(if morphed { &m.morphed } else { &m.normal }),
                _ => None,
            },
            Slot::MacroStep {
                behavior,
                step,
                index,
            } => match &self.behavior(behavior)?.kind {
                BehaviorKind::Macro(m) => match m.steps.get(step)? {
                    MacroStep::Tap(b) | MacroStep::Press(b) | MacroStep::Release(b) => b.get(index),
                    _ => None,
                },
                _ => None,
            },
        }
    }

    pub fn set_slot(&mut self, slot: Slot, binding: Binding) -> Result<(), ModelError> {
        use crate::behavior::MacroStep;
        let target = match slot {
            Slot::Combo(id) => Some(&mut self.combo_mut(id)?.binding),
            Slot::TapDance { behavior, index } => match &mut self.behavior_mut(behavior)?.kind {
                BehaviorKind::TapDance(t) => t.bindings.get_mut(index),
                _ => None,
            },
            Slot::ModMorph { behavior, morphed } => match &mut self.behavior_mut(behavior)?.kind {
                BehaviorKind::ModMorph(m) => Some(if morphed {
                    &mut m.morphed
                } else {
                    &mut m.normal
                }),
                _ => None,
            },
            Slot::MacroStep {
                behavior,
                step,
                index,
            } => match &mut self.behavior_mut(behavior)?.kind {
                BehaviorKind::Macro(m) => match m.steps.get_mut(step) {
                    Some(MacroStep::Tap(b) | MacroStep::Press(b) | MacroStep::Release(b)) => {
                        b.get_mut(index)
                    }
                    _ => None,
                },
                _ => None,
            },
        };
        *target.ok_or(ModelError::NoSuchSlot)? = binding;
        Ok(())
    }

    // Traversal

    /// Every binding in the project, with where it lives.
    pub fn bindings(&self) -> Vec<(Location, &Binding)> {
        let mut all = Vec::new();
        for layer in &self.layers {
            for (position, binding) in layer.bindings.iter().enumerate() {
                all.push((
                    Location::Key {
                        layer: layer.id,
                        position,
                    },
                    binding,
                ));
            }
        }
        for def in &self.behaviors {
            for binding in def.kind.bindings() {
                all.push((Location::Behavior(def.id), binding));
            }
        }
        for combo in &self.combos {
            all.push((Location::Combo(combo.id), &combo.binding));
        }
        all
    }

    pub fn for_each_binding_mut(&mut self, mut f: impl FnMut(Location, &mut Binding)) {
        for layer in &mut self.layers {
            for (position, binding) in layer.bindings.iter_mut().enumerate() {
                f(
                    Location::Key {
                        layer: layer.id,
                        position,
                    },
                    binding,
                );
            }
        }
        for def in &mut self.behaviors {
            for binding in def.kind.bindings_mut() {
                f(Location::Behavior(def.id), binding);
            }
        }
        for combo in &mut self.combos {
            f(Location::Combo(combo.id), &mut combo.binding);
        }
    }

    /// The parameters of a binding at a key, for callers that only read.
    pub fn binding(&self, layer: LayerId, position: usize) -> Option<&Binding> {
        self.layer(layer)?.bindings.get(position)
    }
}
