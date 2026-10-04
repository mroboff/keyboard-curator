//! Imports a `.keymap` file or a MoErgo Layout Editor export, without the
//! app, and prints what happened.
//!
//! Usage: `cargo run -p kc-import --example import -- <board-id> <file> [out.kcproj]`

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(board_id), Some(path)) = (args.next(), args.next()) else {
        return Err("usage: import <board-id> <file> [out.kcproj]".into());
    };
    let boards = kc_boards::built_in()?;
    let board = boards
        .iter()
        .find(|b| b.id == board_id)
        .ok_or_else(|| format!("unknown board `{board_id}`"))?;
    let text = std::fs::read_to_string(&path)?;
    let (project, report) = if path.ends_with(".json") {
        kc_import::import_moergo(&text, board)?
    } else {
        kc_import::import_keymap("Imported", &text, board)?
    };
    println!(
        "{} layers, {} behaviours, {} combos, {} raw bindings",
        report.layers, report.behaviors, report.combos, report.raw_bindings
    );
    println!("kept as text: {:?}", report.raw_blocks);
    for note in &report.notes {
        println!("note: {note}");
    }
    let problems = kc_model::validate(&project, board);
    let errors = problems
        .iter()
        .filter(|p| p.severity == kc_model::Severity::Error)
        .count();
    println!("{errors} error(s), {} warning(s)", problems.len() - errors);
    for problem in problems.iter().take(8) {
        println!("  {:?}: {}", problem.severity, problem.message);
    }
    if let Some(out) = args.next() {
        kc_model::file::save(&project, out.as_ref())?;
    }
    Ok(())
}
