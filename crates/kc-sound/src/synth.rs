//! Makes the sound: one voice per held key, mixed into samples.

/// The shape of the wave a voice plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Waveform {
    /// A pure tone.
    Sine,
    /// Soft and hollow.
    Triangle,
    /// Bright and buzzy.
    Sawtooth,
    /// Hollow and reedy, like an old game console.
    Square,
}

impl Waveform {
    pub const ALL: [Waveform; 4] = [
        Waveform::Sine,
        Waveform::Triangle,
        Waveform::Sawtooth,
        Waveform::Square,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Waveform::Sine => "Sine",
            Waveform::Triangle => "Triangle",
            Waveform::Sawtooth => "Sawtooth",
            Waveform::Square => "Square",
        }
    }

    /// The wave's value at `phase`, which runs from 0 to 1 over one cycle
    /// and advances by `step` per sample. The sharp edges of the sawtooth
    /// and square are rounded over one sample, which keeps high notes from
    /// sounding harsh.
    fn value(self, phase: f32, step: f32) -> f32 {
        match self {
            Waveform::Sine => (phase * std::f32::consts::TAU).sin(),
            Waveform::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
            Waveform::Sawtooth => 2.0 * phase - 1.0 - edge(phase, step),
            Waveform::Square => {
                let level = if phase < 0.5 { 1.0 } else { -1.0 };
                level + edge(phase, step) - edge((phase + 0.5).fract(), step)
            }
        }
    }
}

/// The correction that rounds off a jump at the start of a cycle.
fn edge(phase: f32, step: f32) -> f32 {
    if step <= 0.0 {
        0.0
    } else if phase < step {
        let t = phase / step;
        2.0 * t - t * t - 1.0
    } else if phase > 1.0 - step {
        let t = (phase - 1.0) / step;
        t * t + 2.0 * t + 1.0
    } else {
        0.0
    }
}

/// How long a note takes to reach full volume, and to die away once its
/// key is let go. Long enough that notes do not click.
const ATTACK_SECONDS: f32 = 0.008;
const RELEASE_SECONDS: f32 = 0.18;
/// How loud one voice is, leaving room for a chord.
const VOICE_LEVEL: f32 = 0.22;
/// The most voices sounding at once. A new note beyond this replaces the
/// oldest.
const MAX_VOICES: usize = 24;

struct Voice {
    id: u32,
    frequency: f32,
    phase: f32,
    level: f32,
    held: bool,
}

/// A polyphonic synthesizer. Notes are started and stopped by an ID of the
/// caller's choosing, such as a key's position.
pub struct Synth {
    sample_rate: f32,
    waveform: Waveform,
    volume: f32,
    voices: Vec<Voice>,
}

impl Synth {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate: sample_rate.max(1) as f32,
            waveform: Waveform::Sine,
            volume: 0.6,
            voices: Vec::new(),
        }
    }

    /// Sets the rate samples are made at, once the sound output says
    /// what it wants.
    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate.max(1) as f32;
    }

    pub fn set_waveform(&mut self, waveform: Waveform) {
        self.waveform = waveform;
    }

    /// Sets the volume, from 0 (silent) to 1.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }

    /// Starts a note, or restarts it if `id` is already sounding.
    pub fn note_on(&mut self, id: u32, frequency: f32) {
        if let Some(voice) = self.voices.iter_mut().find(|v| v.id == id) {
            voice.frequency = frequency;
            voice.held = true;
            return;
        }
        if self.voices.len() >= MAX_VOICES {
            self.voices.remove(0);
        }
        self.voices.push(Voice {
            id,
            frequency,
            phase: 0.0,
            level: 0.0,
            held: true,
        });
    }

    /// Lets a note go; it fades out.
    pub fn note_off(&mut self, id: u32) {
        if let Some(voice) = self.voices.iter_mut().find(|v| v.id == id) {
            voice.held = false;
        }
    }

    /// Lets every note go.
    pub fn all_off(&mut self) {
        for voice in &mut self.voices {
            voice.held = false;
        }
    }

    /// How many voices are sounding, including those fading out.
    pub fn sounding(&self) -> usize {
        self.voices.len()
    }

    /// Fills `samples` with the next stretch of sound, one value per
    /// sample, between -1 and 1.
    pub fn render(&mut self, samples: &mut [f32]) {
        let attack = 1.0 / (ATTACK_SECONDS * self.sample_rate);
        let release = 1.0 / (RELEASE_SECONDS * self.sample_rate);
        for sample in samples.iter_mut() {
            let mut mixed = 0.0;
            for voice in &mut self.voices {
                voice.level = if voice.held {
                    (voice.level + attack).min(1.0)
                } else {
                    (voice.level - release).max(0.0)
                };
                let step = voice.frequency / self.sample_rate;
                mixed += self.waveform.value(voice.phase, step) * voice.level;
                voice.phase = (voice.phase + step).fract();
            }
            // Many keys at once are squeezed toward full scale, not cut
            // off at it.
            *sample = (mixed * VOICE_LEVEL * self.volume).tanh();
        }
        self.voices.retain(|v| v.held || v.level > 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn render(synth: &mut Synth, seconds: f32) -> Vec<f32> {
        let mut samples = vec![0.0; (seconds * RATE as f32) as usize];
        synth.render(&mut samples);
        samples
    }

    /// How many times the signal crosses zero going up, which for a simple
    /// wave is its number of cycles.
    fn cycles(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count()
    }

    #[test]
    fn silence_until_a_note_is_played() {
        let mut synth = Synth::new(RATE);
        assert!(render(&mut synth, 0.05).iter().all(|s| *s == 0.0));
        assert_eq!(synth.sounding(), 0);
    }

    #[test]
    fn every_waveform_plays_at_the_pitch_asked_for() {
        for waveform in Waveform::ALL {
            let mut synth = Synth::new(RATE);
            synth.set_waveform(waveform);
            synth.note_on(1, 440.0);
            let samples = render(&mut synth, 1.0);
            let counted = cycles(&samples) as i32;
            assert!((counted - 440).abs() <= 2, "{waveform:?}: {counted}");
            // Loud enough to hear, and never beyond full scale.
            let peak = samples.iter().fold(0f32, |peak, s| peak.max(s.abs()));
            assert!(peak > 0.05 && peak <= 1.0, "{waveform:?}: {peak}");
        }
    }

    #[test]
    fn notes_fade_in_and_out_so_they_do_not_click() {
        let mut synth = Synth::new(RATE);
        synth.note_on(1, 440.0);
        let start = render(&mut synth, 0.02);
        // The first sample is silent and the level grows from there.
        assert!(start[0].abs() < 0.01);
        let biggest_jump = start
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0f32, f32::max);
        assert!(biggest_jump < 0.05, "{biggest_jump}");

        synth.note_off(1);
        // Still sounding just after release, gone soon after.
        render(&mut synth, 0.05);
        assert_eq!(synth.sounding(), 1);
        render(&mut synth, 0.3);
        assert_eq!(synth.sounding(), 0);
        assert!(render(&mut synth, 0.01).iter().all(|s| *s == 0.0));
    }

    #[test]
    fn many_keys_at_once_stay_within_full_scale() {
        let mut synth = Synth::new(RATE);
        synth.set_waveform(Waveform::Square);
        synth.set_volume(1.0);
        for id in 0..40 {
            synth.note_on(id, 110.0 * (1.0 + id as f32 / 7.0));
        }
        // Only so many voices sound; the oldest give way.
        assert_eq!(synth.sounding(), MAX_VOICES);
        assert!(render(&mut synth, 0.2).iter().all(|s| s.abs() <= 1.0));
        synth.all_off();
        render(&mut synth, 0.4);
        assert_eq!(synth.sounding(), 0);
    }

    #[test]
    fn volume_scales_the_sound_and_zero_is_silent() {
        let peak = |volume: f32| {
            let mut synth = Synth::new(RATE);
            synth.set_volume(volume);
            synth.note_on(1, 440.0);
            render(&mut synth, 0.2)
                .iter()
                .fold(0f32, |peak, s| peak.max(s.abs()))
        };
        assert_eq!(peak(0.0), 0.0);
        assert!(peak(1.0) > peak(0.3));
        // Out-of-range volumes are brought into range.
        assert_eq!(peak(5.0), peak(1.0));
    }

    #[test]
    fn playing_a_held_key_again_does_not_stack_voices() {
        let mut synth = Synth::new(RATE);
        synth.note_on(7, 220.0);
        synth.note_on(7, 220.0);
        assert_eq!(synth.sounding(), 1);
    }
}
