//! Stable identifiers. Everything that can be referred to from elsewhere in
//! a project has an ID that never changes when things are reordered or
//! renamed; positions and names are resolved only when emitting.

use serde::{Deserialize, Serialize};

macro_rules! id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u32);
    };
}

id!(
    /// Identifies a layer, independent of its position in the keymap.
    LayerId
);
id!(
    /// Identifies a user-defined behavior such as a hold-tap or macro.
    BehaviorId
);
id!(
    /// Identifies a combo.
    ComboId
);
id!(
    /// Identifies one of the user's saved keyboards.
    KeyboardId
);
