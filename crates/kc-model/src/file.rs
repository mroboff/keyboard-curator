//! The project file: versioned JSON, laid out so that git diffs are readable.

use std::path::Path;

use serde_json::Value;

use crate::project::{Project, FORMAT};

/// The project file extension. Files are JSON.
pub const EXTENSION: &str = "kcproj";

/// Values whose one-line form fits in this many characters stay on one line,
/// which puts each key binding on a line of its own.
const INLINE_WIDTH: usize = 100;

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("not a Keyboard Curator project: {0}")]
    NotAProject(String),
    #[error("this project was saved by a newer version of Keyboard Curator (format {found}; this version reads up to {supported})")]
    Newer { found: u64, supported: u32 },
    #[error("the project file is damaged: {0}")]
    Invalid(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

fn inline(value: &Value, out: &mut String) {
    match value {
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                inline(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (i, (key, item)) in map.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push_str(": ");
                inline(item, out);
            }
            out.push('}');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

fn pretty(value: &Value, depth: usize, out: &mut String) {
    let mut one_line = String::new();
    inline(value, &mut one_line);
    let is_empty = match value {
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => true,
    };
    // The root is always expanded so top-level fields get their own lines.
    if is_empty || (depth > 0 && one_line.len() <= INLINE_WIDTH) {
        out.push_str(&one_line);
        return;
    }
    let indent = "  ".repeat(depth + 1);
    let close = "  ".repeat(depth);
    match value {
        Value::Array(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&indent);
                pretty(item, depth + 1, out);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&close);
            out.push(']');
        }
        Value::Object(map) => {
            out.push_str("{\n");
            for (i, (key, item)) in map.iter().enumerate() {
                out.push_str(&indent);
                out.push_str(&Value::String(key.clone()).to_string());
                out.push_str(": ");
                pretty(item, depth + 1, out);
                out.push_str(if i + 1 < map.len() { ",\n" } else { "\n" });
            }
            out.push_str(&close);
            out.push('}');
        }
        _ => unreachable!("scalars are always written inline"),
    }
}

/// The project as file text.
pub fn to_json(project: &Project) -> String {
    let value = serde_json::to_value(project).expect("a project always serializes");
    let mut out = String::new();
    pretty(&value, 0, &mut out);
    out.push('\n');
    out
}

/// Brings a file written by an older version up to the current format.
/// Each format bump adds one step here.
fn migrate(value: Value, from: u64) -> Result<Value, FileError> {
    match from {
        v if v == u64::from(FORMAT) => Ok(value),
        v => Err(FileError::Invalid(format!("unknown format {v}"))),
    }
}

pub fn from_json(text: &str) -> Result<Project, FileError> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| FileError::NotAProject(e.to_string()))?;
    let found = value
        .get("format")
        .and_then(Value::as_u64)
        .ok_or_else(|| FileError::NotAProject("it has no format version".into()))?;
    if found > u64::from(FORMAT) {
        return Err(FileError::Newer {
            found,
            supported: FORMAT,
        });
    }
    let mut value = migrate(value, found)?;
    value["format"] = FORMAT.into();
    serde_json::from_value(value).map_err(|e| FileError::Invalid(e.to_string()))
}

/// Writes the project, replacing any existing file only once the new
/// contents are safely on disk.
pub fn save(project: &Project, path: &Path) -> Result<(), FileError> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    std::fs::write(&temp, to_json(project))?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

pub fn load(path: &Path) -> Result<Project, FileError> {
    from_json(&std::fs::read_to_string(path)?)
}
