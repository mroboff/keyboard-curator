//! The Keyboard Curator desktop application. This is the only crate allowed
//! to depend on the GUI framework; everything else stays UI-independent.

fn main() {
    println!("Keyboard Curator {}", env!("CARGO_PKG_VERSION"));
}
