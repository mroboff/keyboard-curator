//! Reads a VIA definition file and says what board it would make.
//!
//! Usage: `cargo run -p kc-via --example definition -- <file.json>`

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: definition <file.json>")?;
    let definition = kc_via::definition::parse(&std::fs::read_to_string(path)?)?;
    println!(
        "{} ({:04x}:{:04x}), matrix {}×{}, {} keys drawn, {} options, lighting {:?}, custom keycodes {:?}, modules {:?}",
        definition.name,
        definition.vendor_id,
        definition.product_id,
        definition.rows,
        definition.cols,
        definition.keys.len(),
        definition.options.len(),
        definition.lighting,
        definition.custom_keycodes,
        definition.keycode_modules
    );
    let board = definition.board()?;
    let layout = board.layout(&board.default_layout).unwrap();
    let via = board.firmware[0].via.as_ref().unwrap();
    println!(
        "board {}: {} keys in the default layout, matrix positions {:?}…",
        board.id,
        layout.keys.len(),
        &via.matrix[..via.matrix.len().min(8)]
    );
    for key in layout.keys.iter().take(6) {
        println!("  {:?}", <[i32; 7]>::from(*key));
    }
    Ok(())
}
