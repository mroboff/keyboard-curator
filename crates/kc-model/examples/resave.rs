//! Reads layout files and writes them back in the current format, so that
//! the shipped templates under `templates/` stay current after a format
//! change:
//!
//! `cargo run -p kc-model --example resave -- templates/*.kcproj`

use kc_model::file;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        return Err("usage: resave <layout.kcproj>...".into());
    }
    for path in paths {
        let path = std::path::Path::new(&path);
        let project = file::load(path)?;
        file::save(&project, path)?;
        println!(
            "{}: format {}, {} behaviors ({} templates)",
            path.display(),
            project.format,
            project.behaviors.len(),
            project.behavior_templates().count()
        );
    }
    Ok(())
}
