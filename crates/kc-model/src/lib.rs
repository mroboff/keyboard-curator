//! The project model: layers, bindings, behavior definitions, combos, macros, pointing, lighting and firmware settings, plus the commands that edit them.
//!
//! A [`Project`] is the source of truth for one keyboard's configuration;
//! the ZMK config files are generated from it. Layers and behaviors are
//! referred to by stable IDs, so reordering or renaming never breaks a
//! reference. Edits go through an [`Editor`] for undo and redo, and
//! [`validate`] checks a project against its board.

pub mod behavior;
pub mod binding;
pub mod clipboard;
pub mod edit;
pub mod editor;
pub mod features;
pub mod file;
pub mod ids;
pub mod keyboards;
pub mod keycap;
pub mod lighting;
pub mod picker;
pub mod project;
pub mod text;
pub mod validate;

pub use binding::{BehaviorRef, Binding, KeyExpr, Param};
pub use editor::Editor;
pub use ids::{BehaviorId, ComboId, KeyboardId, LayerId};
pub use keyboards::{Device, Keyboard, Keyboards};
pub use project::{Layer, Location, ModelError, Project, Slot};
pub use validate::{validate, Problem, Severity};
