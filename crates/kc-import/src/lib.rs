//! Best-effort import of existing `.keymap` files and MoErgo Layout Editor JSON into a project.
//!
//! Importing never edits the source file. What the model understands
//! becomes structured; everything else is carried over as raw text, and the
//! [`Report`] says which was which.

pub mod dts;
mod keymap;
mod moergo;

pub use keymap::{import_conf, import_keymap};
pub use moergo::import_moergo;

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
    #[error("this is not a MoErgo Layout Editor export: {0}")]
    NotAnExport(String),
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

/// Imports a file for whichever of `boards` it belongs to: a MoErgo Layout
/// Editor export if it is JSON, otherwise a keymap. A `.conf` file's text,
/// if there is one beside a keymap, is read into settings.
///
/// A keymap does not say which keyboard it is for, so the board is the one
/// the file's name or contents point to, or failing that the first whose
/// layouts fit its key count.
pub fn import_file(
    name: &str,
    text: &str,
    conf: Option<&str>,
    boards: &[kc_boards::Board],
) -> Result<(kc_model::Project, usize, Report), ImportError> {
    let lower = format!("{} {}", name.to_lowercase(), text.to_lowercase());
    let mut order: Vec<usize> = (0..boards.len()).collect();
    // Boards the file mentions come first.
    order.sort_by_key(|index| {
        let board = &boards[*index];
        let mentioned = lower.contains(&board.name.to_lowercase())
            || board
                .pointing
                .iter()
                .any(|d| lower.contains(&d.listener.to_lowercase()))
            || board
                .layouts
                .iter()
                .any(|l| l.id.len() > 16 && lower.contains(&l.id.to_lowercase()));
        !mentioned
    });
    let is_json = text.trim_start().starts_with('{');
    let mut last = ImportError::NoKeymap;
    for index in order {
        let board = &boards[index];
        let stem = name.rsplit('/').next().unwrap_or(name);
        let stem = stem.split('.').next().unwrap_or(stem);
        let result = if is_json {
            import_moergo(text, board)
        } else {
            import_keymap(stem, text, board)
        };
        match result {
            Ok((mut project, report)) => {
                if let Some(conf) = conf {
                    import_conf(&mut project, conf);
                }
                return Ok((project, index, report));
            }
            Err(error) => last = error,
        }
    }
    Err(last)
}

impl Report {
    /// A one-paragraph summary for the user.
    pub fn summary(&self) -> String {
        let mut text = format!(
            "Imported {} layer(s), {} behavior(s) and {} combo(s).",
            self.layers, self.behaviors, self.combos
        );
        if !self.raw_blocks.is_empty() {
            text.push_str(&format!(
                " Kept as custom devicetree: {}.",
                self.raw_blocks.join(", ")
            ));
        }
        for note in &self.notes {
            text.push(' ');
            text.push_str(note);
        }
        text
    }
}
