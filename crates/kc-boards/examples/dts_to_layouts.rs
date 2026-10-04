//! Converts the `zmk,physical-layout` nodes in a devicetree file into
//! `[[layouts]]` tables for a board definition.
//!
//! Usage: `cargo run -p kc-boards --example dts_to_layouts -- path/to/layouts.dtsi`

use kc_boards::dts::parse_physical_layouts;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: dts_to_layouts <file>")?;
    for layout in parse_physical_layouts(&std::fs::read_to_string(path)?)? {
        println!("[[layouts]]");
        println!("id = \"{}\"", layout.label);
        println!("name = \"{}\"", layout.display_name.unwrap_or_default());
        println!("keys = [");
        for key in layout.keys {
            let [w, h, x, y, rot, rx, ry]: [i32; 7] = key.into();
            println!("  [{w}, {h}, {x:>4}, {y:>3}, {rot:>5}, {rx:>4}, {ry:>4}],");
        }
        println!("]\n");
    }
    Ok(())
}
