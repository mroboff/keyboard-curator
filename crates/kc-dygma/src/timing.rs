//! The Curator firmware's per-key timing: a hold and tap timing of a
//! key's own, which is the key's feel in the layout.
//!
//! The format is a fact about the fork (mroboff/NeuronWireless_defy,
//! branch `curator`, CURATOR.md, "Focus commands the fork adds"); no code
//! is taken from it. `timing.map` is a list of 16-bit values, nine per
//! entry: `key layer hold overlap minHold minPrior skHold skOverlap
//! skTimeout`. `key` is the keymap slot counted from 1; `layer` is `0`
//! for every layer, otherwise the layer's number counted from 1. The next
//! four are the values a dual-use key (a hold and tap) takes, in the units
//! of `qukeys.holdTimeout`, `qukeys.overlapThreshold`,
//! `qukeys.minimumHoldTime` and `qukeys.minimumPriorInterval`; the last
//! three are a superkey's, in the units of `superkeys.holdstart`,
//! `superkeys.overlap` and `superkeys.timeout`. A field of `65535` means
//! the keyboard's own value. The list ends with a key of `0`, which is
//! all an empty table is. `timing.capacity` is how many entries the table
//! holds (48), and `timing.defaults` the keyboard's current seven values.
//! Stock Dygma firmware has none of this; the commands are sent only when
//! `help` lists them.

use kc_boards::board::DygmaProfile;
use kc_model::behavior::{HoldTapSettings, StickyKeySettings, TapDanceSettings};
use kc_model::setup::{Choice, KeySetup};
use kc_model::{LayerId, Location, Problem, Project, Severity};

/// How many entries the table holds, when the keyboard does not say.
pub const CAPACITY: usize = 48;
/// A field left to the keyboard's own value.
pub const DEFAULT: u16 = 65535;
/// Values per entry on the wire.
const FIELDS: usize = 9;
/// The most `minPrior` holds: the firmware keeps it in a byte.
const MIN_PRIOR_MAX: u16 = 254;

/// One key's timing, as the keyboard stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// The keymap slot, counted from 1.
    pub key: u8,
    /// `0` for every layer, otherwise the layer's number counted from 1.
    pub layer: u8,
    pub hold: u16,
    pub overlap: u16,
    pub min_hold: u16,
    pub min_prior: u16,
    pub sk_hold: u16,
    pub sk_overlap: u16,
    pub sk_timeout: u16,
}

impl Entry {
    /// An entry for a key that leaves every field to the keyboard.
    pub const fn keyboard(key: u8, layer: u8) -> Self {
        Self {
            key,
            layer,
            hold: DEFAULT,
            overlap: DEFAULT,
            min_hold: DEFAULT,
            min_prior: DEFAULT,
            sk_hold: DEFAULT,
            sk_overlap: DEFAULT,
            sk_timeout: DEFAULT,
        }
    }

    /// Whether every field is the keyboard's own value.
    pub fn is_keyboard(&self) -> bool {
        [
            self.hold,
            self.overlap,
            self.min_hold,
            self.min_prior,
            self.sk_hold,
            self.sk_overlap,
            self.sk_timeout,
        ]
        .iter()
        .all(|v| *v == DEFAULT)
    }
}

/// The table's entries, in order.
pub fn parse(raw: &[u16]) -> Vec<Entry> {
    let mut entries = Vec::new();
    for chunk in raw.as_chunks::<FIELDS>().0 {
        if chunk[0] == 0 {
            break;
        }
        entries.push(Entry {
            key: u8::try_from(chunk[0]).unwrap_or(u8::MAX),
            layer: u8::try_from(chunk[1]).unwrap_or(u8::MAX),
            hold: chunk[2],
            overlap: chunk[3],
            min_hold: chunk[4],
            min_prior: chunk[5],
            sk_hold: chunk[6],
            sk_overlap: chunk[7],
            sk_timeout: chunk[8],
        });
    }
    entries
}

/// The table as the keyboard stores it: every entry, then the `0` that
/// ends the list; an empty table is `0`.
pub fn serialize(entries: &[Entry]) -> Vec<u16> {
    let mut out = Vec::with_capacity(entries.len() * FIELDS + 1);
    for e in entries {
        out.extend_from_slice(&[
            u16::from(e.key),
            u16::from(e.layer),
            e.hold,
            e.overlap,
            e.min_hold,
            e.min_prior,
            e.sk_hold,
            e.sk_overlap,
            e.sk_timeout,
        ]);
    }
    out.push(0);
    out
}

/// The settings a part of a key's feel asks for: none for the keyboard's
/// own timing, which settings of the key's own that still match the
/// defaults count as, like the superkeys do.
fn asked<S>(project: &Project, choice: &Choice<S>) -> Option<S>
where
    S: Clone + Default + PartialEq,
    kc_model::behavior::Settings: TryInto<S>,
{
    let settings = match choice {
        Choice::Default => return None,
        Choice::Custom(s) => s.clone(),
        Choice::Template(id) => project.behavior(*id)?.kind.settings().try_into().ok()?,
    };
    (settings != S::default()).then_some(settings)
}

/// A key's timing as the firmware takes it, with the key and layer left
/// to fill in, and what the firmware cannot honor of the key's feel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timing {
    pub entry: Entry,
    pub unhonored: Vec<String>,
}

/// What a key's feel asks of the firmware. The hold feel's hold time is
/// the key's `hold` (and `skHold`, for a key written as a superkey) and
/// its pause before a hold is `minPrior`; the taps feel's time between
/// taps is `skTimeout`. Everything else stays the keyboard's.
pub fn of_setup(project: &Project, setup: &KeySetup) -> Timing {
    let mut entry = Entry::keyboard(0, 0);
    let mut unhonored = Vec::new();
    if setup.has_hold() {
        if let Some(h) = asked::<HoldTapSettings>(project, &setup.feel.hold) {
            let hold = u16::try_from(h.tapping_term_ms).unwrap_or(DEFAULT - 1);
            entry.hold = hold;
            entry.sk_hold = hold;
            if let Some(prior) = h.require_prior_idle_ms {
                entry.min_prior = u16::try_from(prior)
                    .unwrap_or(MIN_PRIOR_MAX)
                    .min(MIN_PRIOR_MAX);
            }
            let mut lacking = Vec::new();
            if h.flavor != kc_model::behavior::Flavor::default() {
                lacking.push("\"Decide by\"");
            }
            if h.quick_tap_ms.is_some() {
                lacking.push("\"Tap twice quickly to repeat\"");
            }
            if h.retro_tap {
                lacking.push("\"Tap if released with nothing else pressed\"");
            }
            if h.hold_while_undecided {
                lacking.push("\"Hold starts right away\"");
            }
            if h.opposite_hand_hold || !h.hold_trigger_key_positions.is_empty() {
                lacking.push("holding only with chosen keys");
            }
            if h.hold_trigger_on_release {
                lacking.push("\"Decide when the other key is released\"");
            }
            if !lacking.is_empty() {
                unhonored.push(format!(
                    "the Curator firmware keeps one way of deciding hold and tap: the key's hold time applies, and {} does not",
                    lacking.join(", ")
                ));
            }
        }
    }
    if setup.has_taps() {
        if let Some(t) = asked::<TapDanceSettings>(project, &setup.feel.taps) {
            entry.sk_timeout = u16::try_from(t.tapping_term_ms).unwrap_or(DEFAULT - 1);
        }
    }
    if setup.sticky && asked::<StickyKeySettings>(project, &setup.feel.sticky).is_some() {
        unhonored.push(
            "the Curator firmware has one way of letting a sticky key go: this key's sticky feel does not apply"
                .into(),
        );
    }
    Timing { entry, unhonored }
}

/// The table for the layout: an entry for every key on every layer whose
/// feel is not the keyboard's, or why one cannot be held.
pub fn from_project(
    project: &Project,
    profile: &DygmaProfile,
) -> Result<Vec<Entry>, (Location, String)> {
    let mut entries: Vec<Entry> = Vec::new();
    for (index, layer) in project.layers.iter().enumerate() {
        for (position, binding) in layer.bindings.iter().enumerate() {
            let Some(setup) = KeySetup::read(project, binding) else {
                continue;
            };
            let Some(slot) = profile.key_slots.get(position) else {
                continue;
            };
            let timing = of_setup(project, &setup);
            if timing.entry.is_keyboard() {
                continue;
            }
            let location = Location::Key {
                layer: layer.id,
                position,
            };
            if entries.len() == CAPACITY {
                return Err((
                    location,
                    format!(
                        "the keyboard holds a feel of its own for at most {CAPACITY} keys, and this is one more"
                    ),
                ));
            }
            let (Ok(key), Ok(layer)) = (u8::try_from(slot + 1), u8::try_from(index + 1)) else {
                return Err((
                    location,
                    "the key is beyond what the keyboard counts".into(),
                ));
            };
            entries.push(Entry {
                key,
                layer,
                ..timing.entry
            });
        }
    }
    Ok(entries)
}

/// What the firmware cannot honor of the keys' feels, said at each key.
pub fn warnings(project: &Project) -> Vec<Problem> {
    let mut problems = Vec::new();
    for layer in &project.layers {
        for (position, binding) in layer.bindings.iter().enumerate() {
            let Some(setup) = KeySetup::read(project, binding) else {
                continue;
            };
            for message in of_setup(project, &setup).unhonored {
                problems.push(Problem {
                    severity: Severity::Warning,
                    location: Location::Key {
                        layer: layer.id,
                        position,
                    },
                    message,
                });
            }
        }
    }
    problems
}

/// Gives the layout's keys the feel the keyboard's table holds for them,
/// through each key's setup. `defaults` is the keyboard's own seven
/// values, for an entry that sets a pause before a hold without a hold
/// time. Entries for slots or layers the layout does not have, and for
/// keys with nothing to time, are left out and said in the notes.
pub fn apply(
    project: &mut Project,
    entries: &[Entry],
    profile: &DygmaProfile,
    defaults: &[u16],
) -> Vec<String> {
    let layers: Vec<LayerId> = project.layers.iter().map(|l| l.id).collect();
    let mut notes = Vec::new();
    for entry in entries {
        let Some(position) = entry.key.checked_sub(1).and_then(|slot| {
            profile
                .key_slots
                .iter()
                .position(|s| *s == usize::from(slot))
        }) else {
            notes.push(format!(
                "The keyboard times a key the layout does not have (slot {}), so that timing was left out.",
                entry.key
            ));
            continue;
        };
        let on: Vec<LayerId> = match entry.layer {
            0 => layers.clone(),
            n => match layers.get(usize::from(n) - 1) {
                Some(id) => vec![*id],
                None => {
                    notes.push(format!(
                        "The keyboard times key {} on layer {}, which the layout does not have, so that timing was left out.",
                        position + 1,
                        n
                    ));
                    continue;
                }
            },
        };
        for (index, layer) in layers.iter().enumerate() {
            if !on.contains(layer) {
                continue;
            }
            let Some(mut setup) = KeySetup::of(project, *layer, position) else {
                continue;
            };
            let hold = [entry.hold, entry.sk_hold]
                .into_iter()
                .find(|v| *v != DEFAULT)
                .or_else(|| {
                    (entry.min_prior != DEFAULT).then(|| defaults.first().copied().unwrap_or(0))
                })
                .filter(|v| *v != 0 && *v != DEFAULT);
            let mut kept = false;
            if setup.has_hold() {
                if let Some(hold) = hold {
                    let mut settings = match &setup.feel.hold {
                        Choice::Custom(s) => s.clone(),
                        _ => HoldTapSettings::default(),
                    };
                    settings.tapping_term_ms = u32::from(hold);
                    settings.require_prior_idle_ms =
                        (entry.min_prior != DEFAULT).then_some(u32::from(entry.min_prior));
                    setup.feel.hold = Choice::Custom(settings);
                    kept = true;
                }
            }
            if setup.has_taps() && entry.sk_timeout != DEFAULT {
                setup.feel.taps = Choice::Custom(TapDanceSettings {
                    tapping_term_ms: u32::from(entry.sk_timeout),
                });
                kept = true;
            }
            if !kept {
                if entry.layer != 0 || index == 0 {
                    notes.push(format!(
                        "The keyboard times key {} on layer {}, which has no hold or second tap there, so that timing was left out.",
                        position + 1,
                        index + 1
                    ));
                }
                continue;
            }
            if project.apply_setup(*layer, position, &setup).is_err() {
                notes.push(format!(
                    "The timing of key {} on layer {} could not be kept.",
                    position + 1,
                    index + 1
                ));
            }
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_parse_and_serialize_with_their_ending() {
        // Slot 12 on layer 1 holds after 180 ms; slot 20 on every layer
        // allows 300 ms between taps.
        let raw = [
            12u16, 1, 180, 65535, 65535, 65535, 180, 65535, 65535, 20, 0, 65535, 65535, 65535,
            65535, 65535, 65535, 300, 0,
        ];
        let entries = parse(&raw);
        assert_eq!(
            entries,
            [
                Entry {
                    hold: 180,
                    sk_hold: 180,
                    ..Entry::keyboard(12, 1)
                },
                Entry {
                    sk_timeout: 300,
                    ..Entry::keyboard(20, 0)
                }
            ]
        );
        assert_eq!(serialize(&entries), raw);
        // The empty table, and one cut short.
        assert_eq!(parse(&[0]), Vec::<Entry>::new());
        assert_eq!(parse(&[]), Vec::<Entry>::new());
        assert_eq!(serialize(&[]), [0]);
        assert_eq!(parse(&raw[..12]), entries[..1]);
        assert!(Entry::keyboard(1, 0).is_keyboard());
        assert!(!entries[0].is_keyboard());
        // A full table round-trips.
        let full: Vec<Entry> = (1..=CAPACITY)
            .map(|n| Entry {
                hold: 200,
                ..Entry::keyboard(n as u8, 0)
            })
            .collect();
        assert_eq!(parse(&serialize(&full)), full);
        assert_eq!(serialize(&full).len(), CAPACITY * FIELDS + 1);
    }
}
