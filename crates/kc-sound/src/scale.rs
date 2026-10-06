//! Which note each key plays.

/// How notes are laid out across the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scale {
    /// Every semitone in turn along a row; each row up is a fourth higher.
    Chromatic,
    /// The Wicki-Hayden layout: a whole tone to the right, and rows that
    /// alternate a fifth and a fourth above the one below, so that the
    /// notes of a key sit together.
    WickiHayden,
    Major,
    Minor,
    Pentatonic,
    MinorPentatonic,
    Blues,
}

/// The lowest note, played by the bottom-left key: C3.
const BASE: i32 = 48;
/// MIDI notes a piano has, which is as far as anything is transposed.
const LOWEST: i32 = 21;
const HIGHEST: i32 = 108;

impl Scale {
    pub const ALL: [Scale; 7] = [
        Scale::Chromatic,
        Scale::WickiHayden,
        Scale::Major,
        Scale::Minor,
        Scale::Pentatonic,
        Scale::MinorPentatonic,
        Scale::Blues,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Scale::Chromatic => "Chromatic",
            Scale::WickiHayden => "Wicki-Hayden",
            Scale::Major => "Major",
            Scale::Minor => "Minor",
            Scale::Pentatonic => "Pentatonic",
            Scale::MinorPentatonic => "Minor Pentatonic",
            Scale::Blues => "Blues",
        }
    }

    /// The scale's notes within an octave, as semitones above its root.
    fn intervals(self) -> &'static [i32] {
        match self {
            Scale::Chromatic | Scale::WickiHayden => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            Scale::Major => &[0, 2, 4, 5, 7, 9, 11],
            Scale::Minor => &[0, 2, 3, 5, 7, 8, 10],
            Scale::Pentatonic => &[0, 2, 4, 7, 9],
            Scale::MinorPentatonic => &[0, 3, 5, 7, 10],
            Scale::Blues => &[0, 3, 5, 6, 7, 10],
        }
    }

    /// The MIDI note of the key in `column`, counted from the left, and
    /// `row`, counted from the bottom, moved by `transpose` semitones.
    ///
    /// In the Wicki-Hayden layout a step right is a whole tone, and rows
    /// rise by a fifth and a fourth in turn. In every other scale a step
    /// right is the scale's next note, and a row up is about a fourth
    /// higher, so that neighboring keys always sound well together.
    pub fn note(self, column: i32, row: i32, transpose: i32) -> i32 {
        let note = match self {
            Scale::WickiHayden => {
                let octaves = row.div_euclid(2);
                let fifth = if row.rem_euclid(2) == 1 { 7 } else { 0 };
                BASE + 2 * column + 12 * octaves + fifth
            }
            _ => {
                let intervals = self.intervals();
                let count = intervals.len() as i32;
                // A fourth is five of an octave's twelve semitones; a row
                // moves by the nearest whole number of scale steps to that.
                let row_step = (count * 5 + 6) / 12;
                let index = column + row * row_step;
                BASE + 12 * index.div_euclid(count) + intervals[index.rem_euclid(count) as usize]
            }
        };
        fold(note + transpose)
    }
}

/// Brings a note into the range of a piano by whole octaves, so that a
/// large keyboard or a big transposition never plays something inaudible.
fn fold(mut note: i32) -> i32 {
    while note > HIGHEST {
        note -= 12;
    }
    while note < LOWEST {
        note += 12;
    }
    note
}

/// The frequency of a MIDI note in hertz, with A4 at 440.
pub fn frequency(note: i32) -> f32 {
    440.0 * 2f32.powf((note - 69) as f32 / 12.0)
}

/// A note's name, such as `C4` or `F#3`.
pub fn note_name(note: i32) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[note.rem_euclid(12) as usize],
        note.div_euclid(12) - 1
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_have_the_usual_names_and_pitches() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(69), "A4");
        assert_eq!(note_name(48), "C3");
        assert_eq!(note_name(61), "C#4");
        assert!((frequency(69) - 440.0).abs() < 0.001);
        assert!((frequency(57) - 220.0).abs() < 0.001);
        assert!((frequency(60) - 261.626).abs() < 0.01);
    }

    #[test]
    fn the_chromatic_layout_rises_a_semitone_per_key_and_a_fourth_per_row() {
        let c = Scale::Chromatic;
        assert_eq!(note_name(c.note(0, 0, 0)), "C3");
        assert_eq!(c.note(1, 0, 0) - c.note(0, 0, 0), 1);
        assert_eq!(c.note(0, 1, 0) - c.note(0, 0, 0), 5);
        assert_eq!(c.note(3, 2, 0), 48 + 3 + 10);
    }

    #[test]
    fn wicki_hayden_rises_a_tone_per_key_and_a_fifth_then_a_fourth_per_row() {
        let w = Scale::WickiHayden;
        assert_eq!(w.note(1, 0, 0) - w.note(0, 0, 0), 2);
        assert_eq!(w.note(0, 1, 0) - w.note(0, 0, 0), 7);
        assert_eq!(w.note(0, 2, 0) - w.note(0, 1, 0), 5);
        assert_eq!(w.note(0, 2, 0) - w.note(0, 0, 0), 12);
    }

    #[test]
    fn a_scale_plays_only_its_own_notes() {
        for scale in [
            Scale::Major,
            Scale::Minor,
            Scale::Pentatonic,
            Scale::MinorPentatonic,
            Scale::Blues,
        ] {
            let allowed = scale.intervals();
            for row in 0..6 {
                for column in 0..16 {
                    let above_c = (scale.note(column, row, 0) - BASE).rem_euclid(12);
                    assert!(allowed.contains(&above_c), "{scale:?} {column},{row}");
                }
            }
            // Along a row the notes climb, one scale step per key.
            assert!(scale.note(1, 0, 0) > scale.note(0, 0, 0));
            assert!(scale.note(0, 1, 0) > scale.note(0, 0, 0));
        }
        // C major from the bottom-left key: C D E F G A B C.
        let names: Vec<String> = (0..8)
            .map(|c| note_name(Scale::Major.note(c, 0, 0)))
            .collect();
        assert_eq!(names, ["C3", "D3", "E3", "F3", "G3", "A3", "B3", "C4"]);
        // A row up in a major scale starts a fourth higher.
        assert_eq!(note_name(Scale::Major.note(0, 1, 0)), "F3");
    }

    #[test]
    fn transposing_moves_every_note_and_stays_within_a_pianos_range() {
        assert_eq!(Scale::Major.note(0, 0, 12), 60);
        assert_eq!(Scale::Major.note(0, 0, -12), 36);
        for scale in Scale::ALL {
            for transpose in [-48, 0, 48] {
                for row in 0..8 {
                    for column in 0..20 {
                        let note = scale.note(column, row, transpose);
                        assert!((LOWEST..=HIGHEST).contains(&note), "{scale:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn every_scale_has_a_name() {
        let names: Vec<&str> = Scale::ALL.iter().map(|s| s.name()).collect();
        assert_eq!(names.len(), 7);
        assert!(names.iter().all(|n| !n.is_empty()));
    }
}
