//! A small synthesizer for the key tester.
//!
//! Each key of a keyboard gets a note from its place on the board and the
//! chosen [`Scale`], and sounds as one of four classic [`Waveform`]s while
//! it is held. [`Synth`] makes the samples and knows nothing about audio
//! devices; [`Player`] sends them to the computer's sound output, without
//! ever keeping its caller waiting for that output.

mod output;
mod scale;
mod synth;

pub use output::{Player, SoundError, Status};
pub use scale::{frequency, note_name, Scale};
pub use synth::{Synth, Waveform};
