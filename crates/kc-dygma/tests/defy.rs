//! Dygma's Defy: key codes, and layouts to and from a simulated keyboard.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use kc_boards::board::DygmaProfile;
use kc_boards::Board;
use kc_dygma::codec::{decode, encode, kept, NONE, TRANSPARENT};
use kc_dygma::{check, expressible, Focus, FocusError, WRITES};
use kc_model::features::{KeyLight, LockKind, Rgb};
use kc_model::{Binding, KeyExpr, LayerId, Param, Severity};
use kc_zmk::Modifier;

fn defy() -> (Board, DygmaProfile) {
    let board = kc_boards::built_in()
        .unwrap()
        .into_iter()
        .find(|b| b.id == "dygma-defy")
        .unwrap();
    let profile = board.firmware[0].dygma.clone().unwrap();
    (board, profile)
}

/// A keyboard that speaks Focus: it holds values by command name, answers
/// reads, takes writes, and records every line it was sent.
struct Simulated {
    values: BTreeMap<String, String>,
    received: Vec<String>,
    incoming: Vec<u8>,
    outgoing: Vec<u8>,
}

impl Simulated {
    fn new(profile: &DygmaProfile) -> Self {
        let numbers = |count: usize, value: &str| vec![value; count].join(" ");
        let mut values = BTreeMap::new();
        values.insert("version".into(), "v2.2.1".into());
        values.insert("hardware.chip_id".into(), "0123456789abcdef".into());
        values.insert(
            "keymap.custom".into(),
            numbers(profile.layers * profile.slots, "0"),
        );
        // Entry 0 black, entry 1 amber, entry 2 pure white, the rest black.
        let mut palette = vec!["0"; profile.palette * 4];
        palette[4..8].copy_from_slice(&["255", "196", "0", "0"]);
        palette[8..12].copy_from_slice(&["0", "0", "0", "255"]);
        values.insert("palette".into(), palette.join(" "));
        values.insert(
            "colormap.map".into(),
            numbers(profile.layers * profile.leds, "0"),
        );
        Self {
            values,
            received: Vec::new(),
            incoming: Vec::new(),
            outgoing: Vec::new(),
        }
    }

    fn set(&mut self, command: &str, values: &[u16]) {
        let text: Vec<String> = values.iter().map(u16::to_string).collect();
        self.values.insert(command.into(), text.join(" "));
    }
}

impl Write for Simulated {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.incoming.extend_from_slice(bytes);
        while let Some(end) = self.incoming.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.incoming.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line).trim().to_string();
            self.received.push(line.clone());
            let (command, payload) = line.split_once(' ').unwrap_or((&line, ""));
            if payload.is_empty() {
                let value = self.values.get(command).cloned().unwrap_or_default();
                self.outgoing
                    .extend_from_slice(format!("{value} \r\n.\r\n").as_bytes());
            } else {
                self.values.insert(command.to_string(), payload.to_string());
                self.outgoing.extend_from_slice(b".\r\n");
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Read for Simulated {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        // A few bytes at a time, as a serial port delivers them.
        let count = self.outgoing.len().min(buffer.len()).min(7);
        buffer[..count].copy_from_slice(&self.outgoing[..count]);
        self.outgoing.drain(..count);
        Ok(count)
    }
}

fn layers(count: u32) -> Vec<LayerId> {
    (1..=count).map(LayerId).collect()
}

#[test]
fn keys_translate_to_the_firmwares_codes_and_back() {
    let l = layers(10);
    let kp = |key: &str| Binding::kp(KeyExpr::new(key));
    let mt = |hold: &str, tap: &str| {
        Binding::new(
            "mt",
            vec![
                Param::Key(KeyExpr::new(hold)),
                Param::Key(KeyExpr::new(tap)),
            ],
        )
    };
    let cases: Vec<(Binding, u16)> = vec![
        (Binding::none(), NONE),
        (Binding::trans(), TRANSPARENT),
        (kp("A"), 4),
        (kp("ESC"), 41),
        (kp("LSHFT"), 225),
        // Ctrl is 256, Alt 512, AltGr 1024, Shift 2048, OS 4096.
        (
            Binding::kp(KeyExpr::new("C").with(Modifier::LCtrl)),
            256 + 6,
        ),
        (
            Binding::kp(KeyExpr::new("A").with(Modifier::RAlt)),
            1024 + 4,
        ),
        (
            Binding::kp(
                KeyExpr::new("TAB")
                    .with(Modifier::LGui)
                    .with(Modifier::LCtrl),
            ),
            256 + 4096 + 43,
        ),
        // A key whose name means a shifted key carries the shift flag.
        (kp("EXCL"), 2048 + 30),
        (Binding::layer("tog", l[0]), 17408),
        (Binding::layer("mo", l[1]), 17451),
        (Binding::layer("to", l[9]), 17501),
        (
            Binding::new("sk", vec![Param::Key(KeyExpr::new("LCTRL"))]),
            49153,
        ),
        (
            Binding::new("sk", vec![Param::Key(KeyExpr::new("RALT"))]),
            49159,
        ),
        (Binding::layer("sl", l[2]), 49163),
        // Seen on a real Defy: Alt or Enter, Ctrl or Enter, AltGr or backslash.
        (mt("LALT", "RET"), 49721),
        (mt("LCTRL", "RET"), 49209),
        (mt("RALT", "BSLH"), 50754),
        (
            Binding::new(
                "lt",
                vec![Param::Layer(l[0]), Param::Key(KeyExpr::new("A"))],
            ),
            51218 + 4,
        ),
        (
            Binding::new(
                "lt",
                vec![Param::Layer(l[7]), Param::Key(KeyExpr::new("SPACE"))],
            ),
            51218 + 7 * 256 + 44,
        ),
        // Volume up is consumer usage 0xE9.
        (kp("C_VOL_UP"), 0x4800 + 0xE9),
    ];
    for (binding, code) in &cases {
        assert_eq!(encode(binding, &l), Ok(*code), "{binding:?}");
        assert_eq!(&decode(*code, &l), binding, "{code}");
    }
}

#[test]
fn codes_without_a_binding_are_kept_as_they_are() {
    let l = layers(10);
    // A superkey, a macro, a mouse key, an LED key and a wireless key.
    for code in [53984u16, 53853, 20481, 17152, 54109, 20865, 23786] {
        let binding = decode(code, &l);
        assert!(matches!(binding, Binding::Raw { .. }), "{code}");
        assert_eq!(encode(&binding, &l), Ok(code));
    }
    assert_eq!(
        kept(53984),
        Binding::Raw {
            raw: "Superkey 5 #53984".into()
        }
    );
    // Every code reads and writes back as itself, whatever it is.
    for code in (0..=u16::MAX).step_by(7) {
        assert_eq!(encode(&decode(code, &l), &l), Ok(code), "{code}");
    }
    // A layer key for a layer the layout does not have is kept too.
    let two = layers(2);
    assert!(matches!(decode(17455, &two), Binding::Raw { .. }));
}

#[test]
fn what_dygma_firmware_lacks_is_refused_with_a_reason() {
    let l = layers(10);
    let refused = |binding: Binding| encode(&binding, &l).unwrap_err();
    assert!(refused(Binding::new("caps_word", vec![])).contains("nothing for &caps_word"));
    assert!(refused(Binding::Raw {
        raw: "&my_macro".into()
    })
    .contains("devicetree"));
    assert!(refused(Binding::new(
        "mt",
        vec![
            Param::Key(KeyExpr::new("LSHFT")),
            Param::Key(KeyExpr::new("A").with(Modifier::LCtrl)),
        ],
    ))
    .contains("plain key"));
    assert!(
        refused(Binding::new("sk", vec![Param::Key(KeyExpr::new("A"))]))
            .contains("single modifier")
    );
    // One-shot layers and layer-taps reach only the first eight layers.
    assert!(refused(Binding::layer("sl", l[8])).contains("first 8 layers"));
    assert!(refused(Binding::layer("mo", LayerId(99))).contains("no longer exists"));
}

#[test]
fn a_layout_read_from_the_keyboard_writes_nothing_when_applied_unchanged() {
    let (board, profile) = defy();
    let mut keyboard = Simulated::new(&profile);
    // Layer 1: Q on the first key, a superkey on the second; a color under
    // the first key, and the underglow lit in white.
    let mut keymap = vec![0u16; profile.layers * profile.slots];
    keymap[profile.key_slots[0]] = 20;
    keymap[profile.key_slots[1]] = 53984;
    keymap[7] = 1234; // A slot the Defy has no key for.
    keyboard.set("keymap.custom", &keymap);
    let mut colors = vec![0u16; profile.layers * profile.leds];
    colors[profile.key_leds[0]] = 1;
    // Another key shows black through a different palette entry.
    colors[profile.key_leds[2]] = 5;
    // The underglow, on the first layer.
    colors[70..profile.leds].fill(2);
    keyboard.set("colormap.map", &colors);

    let mut focus = Focus::new(keyboard);
    let image = focus.read_image().unwrap();
    assert!(image.fits(&profile));
    assert_eq!(image.version, "v2.2.1");

    let project = image.to_project("From keyboard", &board, &profile).unwrap();
    assert_eq!(project.layers.len(), 10);
    assert_eq!(project.layers[0].name, "Layer 1");
    assert_eq!(
        project.layers[0].bindings[0],
        Binding::kp(KeyExpr::new("Q"))
    );
    assert_eq!(project.layers[0].bindings[1], kept(53984));
    let lighting = &project
        .lighting
        .iter()
        .find(|l| l.layer == project.layers[0].id)
        .unwrap();
    assert_eq!(lighting.keys[0], KeyLight::Color(Rgb(255, 196, 0)));
    assert_eq!(lighting.keys[1], KeyLight::Off);
    assert!(check(&project, &profile).is_empty());

    // Unchanged, it is the same image, and nothing is sent.
    let same = image.with_project(&project, &profile).unwrap();
    assert_eq!(same, image);
    assert!(focus.write_changes(&image, &same).unwrap().is_empty());
}

#[test]
fn applying_a_layout_writes_only_what_changed_and_spares_the_rest() {
    let (board, profile) = defy();
    let mut keyboard = Simulated::new(&profile);
    let mut keymap = vec![0u16; profile.layers * profile.slots];
    keymap[7] = 1234;
    keyboard.set("keymap.custom", &keymap);
    let mut colors = vec![0u16; profile.layers * profile.leds];
    // The underglow, on the first layer.
    colors[70..profile.leds].fill(2);
    keyboard.set("colormap.map", &colors);
    let mut focus = Focus::new(keyboard);
    let before = focus.read_image().unwrap();

    let mut project = before.to_project("Mine", &board, &profile).unwrap();
    let (base, nav) = (project.layers[0].id, project.layers[1].id);
    project
        .set_binding(base, 0, Binding::kp(KeyExpr::new("A")))
        .unwrap();
    project
        .set_binding(base, 1, Binding::layer("mo", nav))
        .unwrap();
    // One color the palette has, and one it does not.
    project.lighting_mut(base).unwrap().keys[0] = KeyLight::Color(Rgb(255, 196, 0));
    project.lighting_mut(base).unwrap().keys[1] = KeyLight::Color(Rgb(10, 200, 30));

    let after = before.with_project(&project, &profile).unwrap();
    assert_eq!(after.keymap[profile.key_slots[0]], 4);
    assert_eq!(after.keymap[profile.key_slots[1]], 17451);
    // The slot that is not a key, and the underglow, are untouched.
    assert_eq!(after.keymap[7], 1234);
    assert!(after.colormap[70..profile.leds].iter().all(|c| *c == 2));
    // The existing amber entry is reused; the new green takes a free
    // entry, with the part all channels share moved to white.
    assert_eq!(after.colormap[profile.key_leds[0]], 1);
    let green = usize::from(after.colormap[profile.key_leds[1]]);
    assert!(green != 0 && green != 1 && green != 2);
    assert_eq!(after.palette[green * 4..green * 4 + 4], [0, 190, 20, 10]);
    // Entries in use elsewhere keep their colors.
    assert_eq!(after.palette[..12], before.palette[..12]);

    let sent = focus.write_changes(&before, &after).unwrap();
    assert_eq!(sent, ["palette", "colormap.map", "keymap.custom"]);
    // The keyboard now holds exactly the new image, and reads back as it.
    assert_eq!(focus.read_image().unwrap(), after);
}

#[test]
fn only_exact_safe_commands_ever_reach_the_keyboard() {
    let (board, profile) = defy();
    let mut focus = Focus::new(Simulated::new(&profile));
    let before = focus.read_image().unwrap();
    let mut project = before.to_project("Mine", &board, &profile).unwrap();
    let base = project.layers[0].id;
    project
        .set_binding(base, 0, Binding::kp(KeyExpr::new("A")))
        .unwrap();
    let after = before.with_project(&project, &profile).unwrap();
    focus.write_changes(&before, &after).unwrap();

    // An image of the wrong size is refused before anything is sent.
    let mut short = after.clone();
    short.keymap.pop();
    assert!(matches!(
        focus.write_changes(&after, &short),
        Err(FocusError::Mismatch)
    ));
    assert!(matches!(
        short.with_project(&project, &profile),
        Err(FocusError::Mismatch)
    ));

    // Everything the keyboard was ever sent: reads without arguments, and
    // writes that are one of the three allowed commands with a payload of
    // exactly the size the keyboard holds.
    let keyboard = focus.into_inner();
    let sizes = [
        ("keymap.custom", profile.layers * profile.slots),
        ("palette", profile.palette * 4),
        ("colormap.map", profile.layers * profile.leds),
    ];
    let mut writes = 0;
    for line in &keyboard.received {
        let mut parts = line.split(' ');
        let command = parts.next().unwrap();
        let payload = parts.count();
        if payload == 0 {
            continue;
        }
        writes += 1;
        assert!(WRITES.contains(&command), "{command}");
        let size = sizes.iter().find(|(name, _)| *name == command).unwrap().1;
        assert_eq!(payload, size, "{command}");
    }
    // Only the keymap differed, so only the keymap was written.
    assert_eq!(writes, 1);
    for dangerous in [
        "eeprom",
        "upgrade",
        "hardware.flash",
        "wireless",
        "macros",
        "superkeys",
        "settings",
    ] {
        assert!(!keyboard
            .received
            .iter()
            .any(|line| line.starts_with(dangerous)));
    }
}

#[test]
fn a_layout_dygma_firmware_cannot_hold_is_reported_and_not_written() {
    let (board, profile) = defy();
    let mut focus = Focus::new(Simulated::new(&profile));
    let image = focus.read_image().unwrap();
    let mut project = image.to_project("Mine", &board, &profile).unwrap();
    let base = project.layers[0].id;

    project
        .set_binding(base, 3, Binding::new("caps_word", vec![]))
        .unwrap();
    assert!(!expressible(&Binding::new("caps_word", vec![]), &project));
    assert!(expressible(&Binding::kp(KeyExpr::new("A")), &project));
    project.lighting_mut(base).unwrap().keys[0] = KeyLight::Lock {
        lock: LockKind::Caps,
        off: Rgb(0, 0, 0),
        on: Rgb(255, 0, 0),
    };
    project.add_layer("Eleventh").unwrap();
    let problems = check(&project, &profile);
    assert!(problems.iter().all(|p| p.severity == Severity::Error));
    let said = |text: &str| problems.iter().any(|p| p.message.contains(text));
    assert!(said("nothing for &caps_word"));
    assert!(said("no lock or battery lights"));
    assert!(said("11 layers, and this keyboard holds 10"));
    assert!(matches!(
        image.with_project(&project, &profile),
        Err(FocusError::Unfit(_))
    ));

    // More colors than the palette holds.
    let mut colorful = image.to_project("Colors", &board, &profile).unwrap();
    let base = colorful.layers[0].id;
    for (index, light) in colorful
        .lighting_mut(base)
        .unwrap()
        .keys
        .iter_mut()
        .enumerate()
        .take(17)
    {
        *light = KeyLight::Color(Rgb(10 + index as u8, 0, 0));
    }
    assert!(check(&colorful, &profile)
        .iter()
        .any(|p| p.message.contains("17 different colors")));
}

#[test]
fn a_silent_keyboard_is_reported_as_one() {
    struct Silent;
    impl Write for Silent {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Read for Silent {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "timed out",
            ))
        }
    }
    let error = Focus::new(Silent).read_image().unwrap_err();
    assert!(matches!(error, FocusError::Timeout(_)));
    assert!(error.to_string().contains("Bazecor"));
}
