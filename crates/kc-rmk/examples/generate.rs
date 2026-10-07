//! Writes the RMK firmware repository for a board with a small layout, to
//! build by hand: `cargo run -p kc-rmk --example generate -- <board-id> <dir>`

use kc_model::{Binding, KeyExpr, Param, Project};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(board_id), Some(out)) = (args.next(), args.next()) else {
        return Err("usage: generate <board-id> <output-dir>".into());
    };
    let boards = kc_boards::built_in()?;
    let board = boards
        .iter()
        .find(|b| b.id == board_id)
        .ok_or("unknown board")?;
    let profile = board
        .firmware
        .iter()
        .find(|p| p.rmk.is_some())
        .ok_or("the board has no RMK firmware")?;

    // The board's starter keys, a second layer, and one of each kind of
    // key RMK takes.
    let mut project = Project::new("RMK check", board);
    let base = project.layers[0].id;
    let nav = project.add_layer("Nav")?;
    let key = |name: &str| Param::Key(KeyExpr::new(name));
    let samples = [
        Binding::layer("mo", nav),
        Binding::new("mt", vec![key("LSHFT"), key("A")]),
        Binding::new("lt", vec![Param::Layer(nav), key("SPACE")]),
        Binding::new("sk", vec![key("LCTRL")]),
        Binding::kp(KeyExpr::new("EXCL")),
        Binding::kp(KeyExpr::new("C_VOL_UP")),
        Binding::new("bootloader", vec![]),
        Binding::none(),
    ];
    for (position, binding) in samples.into_iter().enumerate() {
        project.set_binding(base, position, binding)?;
    }
    project.add_combo("Escape", vec![13, 14], Binding::kp(KeyExpr::new("ESC")));

    let firmware = kc_model::FirmwareConfig::new(profile.id.clone());
    for (path, contents) in kc_rmk::generate(&project, board, profile, &firmware)? {
        let path = std::path::Path::new(&out).join(path);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, contents)?;
        println!("{}", path.display());
    }
    Ok(())
}
