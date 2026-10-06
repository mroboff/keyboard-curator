//! Sends the synthesizer's sound to the computer's audio output.

use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use crate::synth::{Synth, Waveform};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SoundError {
    #[error("this computer has no sound output to play through")]
    NoDevice,
    #[error("the sound output could not be opened: {0}")]
    Unavailable(String),
}

/// How far a [`Player`] has got with the sound output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// The output is being opened. Notes are not heard yet.
    Starting,
    Playing,
    Failed(SoundError),
}

/// A synthesizer playing through the default sound output. Sound stops
/// when it is dropped.
///
/// The output is opened and held on a thread of its own, because the
/// system can take its time answering, and on a bad day never answers:
/// the caller is never kept waiting, and asks [`Player::status`] instead.
pub struct Player {
    synth: Arc<Mutex<Synth>>,
    status: Arc<Mutex<Status>>,
    /// Dropping this ends the sound thread, which closes the output.
    _stop: mpsc::Sender<()>,
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    synth: Arc<Mutex<Synth>>,
) -> Result<cpal::Stream, SoundError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels).max(1);
    let mut mono: Vec<f32> = Vec::new();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                mono.resize(data.len() / channels, 0.0);
                lock(&synth).render(&mut mono);
                // The same sound on every channel.
                for (frame, value) in data.chunks_mut(channels).zip(&mono) {
                    frame.fill(T::from_sample(*value));
                }
            },
            // A glitch or a changed device is not worth interrupting the
            // user for; the sound simply stops or stutters.
            |_| {},
            None,
        )
        .map_err(|e| SoundError::Unavailable(e.to_string()))
}

/// Opens the default sound output and starts it playing the synthesizer.
fn open(synth: &Arc<Mutex<Synth>>) -> Result<cpal::Stream, SoundError> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or(SoundError::NoDevice)?;
    let supported = device
        .default_output_config()
        .map_err(|e| SoundError::Unavailable(e.to_string()))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    lock(synth).set_sample_rate(config.sample_rate);
    let stream = match format {
        SampleFormat::F32 => stream::<f32>(&device, config, synth.clone()),
        SampleFormat::I16 => stream::<i16>(&device, config, synth.clone()),
        SampleFormat::U16 => stream::<u16>(&device, config, synth.clone()),
        SampleFormat::I32 => stream::<i32>(&device, config, synth.clone()),
        other => Err(SoundError::Unavailable(format!(
            "the output wants {other} samples, which this app does not make"
        ))),
    }?;
    stream
        .play()
        .map_err(|e| SoundError::Unavailable(e.to_string()))?;
    Ok(stream)
}

impl Player {
    /// Starts opening the default sound output, and returns at once.
    pub fn new() -> Self {
        let synth = Arc::new(Mutex::new(Synth::new(48_000)));
        let status = Arc::new(Mutex::new(Status::Starting));
        let (stop, stopped) = mpsc::channel::<()>();
        let spawned = {
            let (synth, status) = (synth.clone(), status.clone());
            std::thread::Builder::new()
                .name("kc-sound".to_string())
                .spawn(move || match open(&synth) {
                    Ok(stream) => {
                        *lock(&status) = Status::Playing;
                        // Hold the output open until the player is dropped.
                        let _ = stopped.recv();
                        drop(stream);
                    }
                    Err(error) => *lock(&status) = Status::Failed(error),
                })
        };
        if let Err(error) = spawned {
            *lock(&status) = Status::Failed(SoundError::Unavailable(error.to_string()));
        }
        Self {
            synth,
            status,
            _stop: stop,
        }
    }

    pub fn status(&self) -> Status {
        lock(&self.status).clone()
    }

    /// Starts a note. Before the output is open nothing is started, so
    /// that no note is left waiting to sound late.
    pub fn note_on(&self, id: u32, frequency: f32) {
        if self.status() == Status::Playing {
            lock(&self.synth).note_on(id, frequency);
        }
    }

    pub fn note_off(&self, id: u32) {
        lock(&self.synth).note_off(id);
    }

    pub fn all_off(&self) {
        lock(&self.synth).all_off();
    }

    pub fn set_waveform(&self, waveform: Waveform) {
        lock(&self.synth).set_waveform(waveform);
    }

    /// Sets the volume, from 0 (silent) to 1.
    pub fn set_volume(&self, volume: f32) {
        lock(&self.synth).set_volume(volume);
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}
