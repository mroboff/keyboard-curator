//! The project model: layers, bindings, behaviour definitions, combos, macros, pointing, lighting and firmware settings, plus the commands that edit them.
//!
//! A [`Project`] is the source of truth for one keyboard's configuration;
//! the ZMK config files are generated from it. Layers and behaviours are
//! referred to by stable IDs, so reordering or renaming never breaks a
//! reference. Edits go through an [`Editor`] for undo and redo, and
//! [`validate`] checks a project against its board.

pub mod behavior;
pub mod binding;
pub mod editor;
pub mod features;
pub mod file;
pub mod ids;
pub mod keycap;
pub mod project;
pub mod validate;

pub use binding::{BehaviorRef, Binding, KeyExpr, Param};
pub use editor::Editor;
pub use ids::{BehaviorId, ComboId, LayerId};
pub use project::{Layer, Location, ModelError, Project};
pub use validate::{validate, Problem, Severity};
