//! Plays one octave of a scale on each waveform, to check the sound output.
//!
//! ```sh
//! cargo run -p kc-sound --example scale
//! ```

use std::thread::sleep;
use std::time::{Duration, Instant};

use kc_sound::{frequency, note_name, Player, Scale, Status, Waveform};

fn main() {
    let player = Player::new();
    let asked = Instant::now();
    loop {
        match player.status() {
            Status::Playing => break,
            Status::Failed(error) => {
                eprintln!("No sound: {error}.");
                std::process::exit(1);
            }
            Status::Starting if asked.elapsed() > Duration::from_secs(5) => {
                eprintln!("No sound: the sound output has not answered in 5 seconds.");
                std::process::exit(2);
            }
            Status::Starting => sleep(Duration::from_millis(20)),
        }
    }
    println!(
        "Sound output open after {} ms.",
        asked.elapsed().as_millis()
    );
    player.set_volume(0.4);
    for waveform in Waveform::ALL {
        player.set_waveform(waveform);
        println!("{}", waveform.name());
        for column in 0..8 {
            let note = Scale::Major.note(column, 1, 0);
            println!("  {}", note_name(note));
            player.note_on(column as u32, frequency(note));
            sleep(Duration::from_millis(140));
            player.note_off(column as u32);
        }
        sleep(Duration::from_millis(250));
    }
}
