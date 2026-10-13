//! Writes a board file for every keyboard shield in a ZMK checkout that
//! declares a physical layout.
//!
//! Usage: `cargo run -p kc-import --example zmk_boards -- <zmk checkout> <revision> <out dir>`
//!
//! The checkout must be at `revision` (a tag such as `v0.3.0`), which the
//! firmware profiles pin. Files that exist already are overwritten.

use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [zmk, revision, out] = args.as_slice() else {
        return Err("usage: zmk_boards <zmk checkout> <revision> <out dir>".into());
    };
    let out = Path::new(out);
    std::fs::create_dir_all(out)?;
    let output = kc_import::zmk_tree::boards(Path::new(zmk), revision)?;
    for warning in &output.warnings {
        eprintln!("warning: {warning}");
    }
    for board in &output.boards {
        std::fs::write(out.join(format!("{}.toml", board.id)), &board.toml)?;
        println!("{}", board.id);
    }
    println!("{} boards", output.boards.len());
    Ok(())
}
