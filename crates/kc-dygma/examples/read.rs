//! Reads the connected Dygma keyboard and reports what it holds. It only
//! reads: nothing is written to the keyboard.
//!
//! `cargo run -p kc-dygma --example read`

use kc_model::Binding;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let boards = kc_boards::built_in()?;
    let board = boards
        .iter()
        .find(|b| b.id == "dygma-defy")
        .ok_or("no Defy board definition")?;
    let profile = board.firmware[0].dygma.as_ref().ok_or("no Dygma profile")?;
    let image = kc_dygma::read(board, None)?;
    println!(
        "firmware {}, {} key codes, {} palette values, {} lights",
        image.version,
        image.keymap.len(),
        image.palette.len(),
        image.colormap.len()
    );
    println!(
        "sizes are what the board definition expects: {}",
        image.fits(profile)
    );
    let project = image.to_project("From keyboard", board, profile)?;
    let (mut read, mut kept) = (0, 0);
    for binding in project.layers.iter().flat_map(|l| &l.bindings) {
        match binding {
            Binding::Raw { .. } => kept += 1,
            _ => read += 1,
        }
    }
    println!("{read} keys read as bindings, {kept} kept as their codes");
    println!("problems: {}", kc_dygma::check(&project, profile).len());
    let again = image.with_project(&project, profile)?;
    println!(
        "applying it unchanged would write nothing: {}",
        again == image
    );
    Ok(())
}
