//! Best-effort import of existing `.keymap` files and MoErgo Layout Editor JSON into a project.
//!
//! Importing never edits the source file. What the model understands
//! becomes structured; everything else is carried over as raw text, and the
//! [`Report`] says which was which.

pub mod dts;
mod keymap;

pub use keymap::{import_conf, import_keymap};

/// What an import did, for showing to the user.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub layers: usize,
    pub behaviors: usize,
    pub combos: usize,
    /// Bindings kept as raw text because they could not be read.
    pub raw_bindings: usize,
    /// Parts of the file kept as raw devicetree, by name.
    pub raw_blocks: Vec<String>,
    /// Things the user should know or check.
    pub notes: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ImportError {
    #[error("the file could not be read as devicetree: {0}")]
    Syntax(#[from] dts::DtsError),
    #[error("the file has no keymap")]
    NoKeymap,
    #[error(
        "a layer has {found} keys, which matches none of this keyboard's layouts ({expected})"
    )]
    KeyCount { found: usize, expected: String },
    #[error("layers have different numbers of keys ({0} and {1})")]
    UnevenLayers(usize, usize),
    #[error("{0}")]
    Model(#[from] kc_model::ModelError),
}
