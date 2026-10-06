//! Undo and redo. Every change to a project goes through an [`Editor`], so
//! the UI, import and live editing share one path.
//!
//! History is kept as snapshots of the project rather than as inverse
//! operations: a project is a few kilobytes, and a snapshot can never get
//! an inverse wrong.

use crate::project::{ModelError, Project};

/// The most undo steps kept.
const HISTORY_LIMIT: usize = 500;

/// A saved revision no project state has, for changes made outside the
/// undo history.
const UNSAVED: u64 = u64::MAX;

#[derive(Debug, Clone)]
struct Snapshot {
    label: String,
    project: Project,
    revision: u64,
}

#[derive(Debug, Clone)]
pub struct Editor {
    project: Project,
    revision: u64,
    next_revision: u64,
    saved_revision: u64,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// While a group is open, its edits collapse into one undo step. The
    /// flag records whether that step has been pushed yet.
    group: Option<(String, bool)>,
}

impl Editor {
    pub fn new(project: Project) -> Self {
        Self {
            project,
            revision: 0,
            next_revision: 1,
            saved_revision: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            group: None,
        }
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Applies a change as one undo step named `label`. If `change` fails,
    /// the project is left exactly as it was. A change that alters nothing
    /// records nothing.
    pub fn edit<T>(
        &mut self,
        label: &str,
        change: impl FnOnce(&mut Project) -> Result<T, ModelError>,
    ) -> Result<T, ModelError> {
        let before = self.project.clone();
        let value = match change(&mut self.project) {
            Ok(value) => value,
            Err(error) => {
                self.project = before;
                return Err(error);
            }
        };
        if self.project == before {
            return Ok(value);
        }
        let label = match &mut self.group {
            Some((_, true)) => None,
            Some((label, pushed)) => {
                *pushed = true;
                Some(label.clone())
            }
            None => Some(label.to_string()),
        };
        if let Some(label) = label {
            self.undo.push(Snapshot {
                label,
                project: before,
                revision: self.revision,
            });
            if self.undo.len() > HISTORY_LIMIT {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.revision = self.next_revision;
        self.next_revision += 1;
        Ok(value)
    }

    /// Starts collapsing edits into a single undo step, for gestures such as
    /// painting across many keys. Call [`Editor::end_group`] when it ends.
    pub fn begin_group(&mut self, label: &str) {
        if self.group.is_none() {
            self.group = Some((label.to_string(), false));
        }
    }

    pub fn end_group(&mut self) {
        self.group = None;
    }

    fn step(&mut self, undo: bool) -> Option<String> {
        self.group = None;
        let (from, to) = if undo {
            (&mut self.undo, &mut self.redo)
        } else {
            (&mut self.redo, &mut self.undo)
        };
        let snapshot = from.pop()?;
        to.push(Snapshot {
            label: snapshot.label.clone(),
            project: std::mem::replace(&mut self.project, snapshot.project),
            revision: self.revision,
        });
        self.revision = snapshot.revision;
        Some(snapshot.label)
    }

    /// Undoes the last step and returns its label.
    pub fn undo(&mut self) -> Option<String> {
        self.step(true)
    }

    pub fn redo(&mut self) -> Option<String> {
        self.step(false)
    }

    /// The label of the step [`Editor::undo`] would undo.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    /// Switches the project to another firmware, in every step of its
    /// history too: the firmware belongs to the keyboard the project is
    /// open under, so undo never brings the old one back. The project then
    /// differs from what is on disk.
    pub fn set_firmware(&mut self, firmware: &str) {
        if self.project.firmware == firmware {
            return;
        }
        let history = self.undo.iter_mut().chain(&mut self.redo);
        for project in std::iter::once(&mut self.project).chain(history.map(|s| &mut s.project)) {
            project.firmware = firmware.to_string();
        }
        self.saved_revision = UNSAVED;
    }

    /// Records that the project as it stands now is what is on disk.
    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }

    /// Whether the project differs from what was last saved. Undoing back to
    /// the saved state makes it clean again.
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }
}
