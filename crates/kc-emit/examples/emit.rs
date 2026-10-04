//! Writes the zmk-config files for a project, without the app.
//!
//! Usage: `cargo run -p kc-emit --example emit -- <project.kcproj> <output-dir>`

use kc_model::file;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(project), Some(out)) = (args.next(), args.next()) else {
        return Err("usage: emit <project.kcproj> <output-dir>".into());
    };
    let project = file::load(project.as_ref())?;
    let boards = kc_boards::built_in()?;
    let board = boards
        .iter()
        .find(|b| b.id == project.board)
        .ok_or_else(|| format!("unknown board `{}`", project.board))?;
    for generated in kc_emit::generate(&project, board)? {
        let path = std::path::Path::new(&out).join(&generated.path);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, generated.contents)?;
        println!("{}", path.display());
    }
    Ok(())
}
