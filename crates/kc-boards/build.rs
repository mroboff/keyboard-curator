//! Embeds every board file under `boards/`, so that adding a board is
//! adding a file.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "toml") {
            files.push(path);
        }
    }
}

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("boards");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    walk(&root, &mut files);
    files.sort();
    let mut out = String::from(
        "/// Every board file under `boards/`, in path order.\npub const BUILT_IN: &[&str] = &[\n",
    );
    for file in files {
        out.push_str(&format!("    include_str!({:?}),\n", file.display()));
    }
    out.push_str("];\n");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out_dir.join("built_in.rs"), out).unwrap();
}
