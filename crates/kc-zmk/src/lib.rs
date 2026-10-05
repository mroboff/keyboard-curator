//! ZMK knowledge as data: keycodes and legends, the behaviour catalogue with typed parameters, and the Kconfig option catalogue.

pub mod behaviors;
pub mod feature;
pub mod headers;
pub mod keycodes;
pub mod modifiers;
pub mod pointing;
pub mod settings;

pub use feature::Feature;
pub use modifiers::Modifier;
