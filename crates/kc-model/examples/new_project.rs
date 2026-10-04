//! Writes a new project file for a board, without the app.
//!
//! Usage: `cargo run -p kc-model --example new_project -- <board-id> <path>`

use kc_model::{file, Project};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(board_id), Some(path)) = (args.next(), args.next()) else {
        return Err("usage: new_project <board-id> <path>".into());
    };
    let boards = kc_boards::built_in()?;
    let board = boards
        .iter()
        .find(|b| b.id == board_id)
        .ok_or_else(|| format!("unknown board `{board_id}`"))?;
    let project = Project::from_template(format!("My {}", board.name), board);
    file::save(&project, path.as_ref())?;
    Ok(())
}
