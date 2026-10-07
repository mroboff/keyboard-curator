//! Board definitions as embedded data: physical layouts, variants, LED maps, firmware profiles, limits and bootloader details.
//!
//! Each board is a TOML file under `boards/`, compiled into the binary and
//! validated when loaded. See [`board::Board`] for the format.

pub mod board;
pub mod dts;
pub mod geometry;

pub use board::{Board, BoardError, Delivery, Family};

const BUILT_IN: &[&str] = &[
    include_str!("../boards/cyboard-imprint.toml"),
    include_str!("../boards/moergo-go60.toml"),
    include_str!("../boards/moergo-glove80.toml"),
    include_str!("../boards/dygma-defy.toml"),
];

/// Every board that ships with the app.
pub fn built_in() -> Result<Vec<Board>, BoardError> {
    BUILT_IN.iter().map(|src| Board::from_toml(src)).collect()
}
