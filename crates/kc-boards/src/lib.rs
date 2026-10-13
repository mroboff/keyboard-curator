//! Board definitions as embedded data: physical layouts, variants, LED maps, firmware profiles, limits and bootloader details.
//!
//! Each board is a TOML file under `boards/`, compiled into the binary and
//! validated when loaded. See [`board::Board`] for the format.

pub mod board;
pub mod dts;
pub mod geometry;

use std::sync::OnceLock;

use serde::Deserialize;

pub use board::{Board, BoardError, Controller, Delivery, Family};

// Written by the build script: one `include_str!` per file under `boards/`.
include!(concat!(env!("OUT_DIR"), "/built_in.rs"));

const CONTROLLERS: &str = include_str!("../controllers.toml");

#[derive(Deserialize)]
struct ControllerTable {
    controllers: Vec<Controller>,
}

/// The controller boards that shields are built for.
pub fn controllers() -> &'static [Controller] {
    static TABLE: OnceLock<Vec<Controller>> = OnceLock::new();
    TABLE.get_or_init(|| {
        toml::from_str::<ControllerTable>(CONTROLLERS)
            .expect("the controller table is valid")
            .controllers
    })
}

/// A controller by its ID.
pub fn controller(id: &str) -> Option<&'static Controller> {
    controllers().iter().find(|c| c.id == id)
}

/// Every board that ships with the app.
pub fn built_in() -> Result<Vec<Board>, BoardError> {
    BUILT_IN.iter().map(|src| Board::from_toml(src)).collect()
}
