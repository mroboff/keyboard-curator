//! Fetches the Curator firmware release the Defy's fork profile pins,
//! checks it the way the board page does, and writes it to a folder.
//!
//!     cargo run -p kc-firmware --example fetch_defy_fork -- <folder>

use kc_model::FirmwareConfig;

fn main() {
    let folder = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "defy-curator".into());
    let boards = kc_boards::built_in().expect("boards");
    let board = boards
        .iter()
        .find(|b| b.id == "dygma-defy")
        .expect("the Defy");
    let profile = board
        .firmware
        .iter()
        .find(|p| p.dygma.as_ref().is_some_and(|d| d.fork.is_some()))
        .expect("a fork profile");
    let config = FirmwareConfig::new(&profile.id);
    let token = kc_build::github::find_token();
    match kc_firmware::dygma_fork::fetch(board, &config, token.as_deref()) {
        Ok(firmware) => {
            println!(
                "{} {}: built from {} with plugins {}",
                firmware.fork_name,
                firmware.tag,
                firmware.manifest.source.commit,
                firmware.manifest.kaleidoscope.commit
            );
            let image = kc_dygma::flash::parse_hex(&firmware.neuron_hex).expect("checked");
            println!(
                "Neuron image {:#x}-{:#x} ({} bytes); keyscanner {} bytes",
                image.start,
                image.end(),
                image.bytes.len(),
                firmware.keyscanner.as_ref().map_or(0, Vec::len)
            );
            match kc_firmware::dygma_fork::save(&firmware, std::path::Path::new(&folder)) {
                Ok(path) => println!("Kept in {}", path.display()),
                Err(e) => eprintln!("could not keep the files: {e}"),
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
