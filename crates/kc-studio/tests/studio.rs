//! The Studio client against a simulated keyboard.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::rc::Rc;

use kc_model::{Binding, KeyExpr, Param, Project};
use kc_studio::codec::{decode, decode_key, encode, encode_key};
use kc_studio::framing::{encode as frame, Decoder};
use kc_studio::proto::{self, behaviors, core, keymap, meta, request, request_response, response};
use kc_studio::{compare, BehaviorTable, Client, DeviceBehavior, StudioError};
use prost::Message;

/// What the simulated keyboard holds.
struct Keyboard {
    unlocked: bool,
    names: Vec<&'static str>,
    live: keymap::Keymap,
    saved: keymap::Keymap,
}

/// A stream whose other end is a simulated keyboard: requests written to
/// it are answered into the read side.
#[derive(Clone)]
struct Wire {
    keyboard: Rc<RefCell<Keyboard>>,
    decoder: Rc<RefCell<Decoder>>,
    outgoing: Rc<RefCell<VecDeque<u8>>>,
}

impl Wire {
    fn answer(&self, request: proto::Request) -> request_response::Subsystem {
        let mut keyboard = self.keyboard.borrow_mut();
        let locked = || {
            request_response::Subsystem::Meta(meta::Response {
                response_type: Some(meta::response::ResponseType::SimpleError(
                    meta::ErrorConditions::UnlockRequired as i32,
                )),
            })
        };
        match request.subsystem.unwrap() {
            request::Subsystem::Core(core::Request { request_type }) => {
                let response = match request_type.unwrap() {
                    core::request::RequestType::GetDeviceInfo(_) => {
                        core::response::ResponseType::GetDeviceInfo(core::GetDeviceInfoResponse {
                            name: "Test Board".into(),
                            serial_number: vec![1, 2, 3],
                        })
                    }
                    core::request::RequestType::GetLockState(_) => {
                        core::response::ResponseType::GetLockState(keyboard.unlocked as i32)
                    }
                    _ => unimplemented!(),
                };
                request_response::Subsystem::Core(core::Response {
                    response_type: Some(response),
                })
            }
            _ if !keyboard.unlocked => locked(),
            request::Subsystem::Behaviors(behaviors::Request { request_type }) => {
                let response = match request_type.unwrap() {
                    behaviors::request::RequestType::ListAllBehaviors(_) => {
                        behaviors::response::ResponseType::ListAllBehaviors(
                            behaviors::ListAllBehaviorsResponse {
                                // IDs deliberately do not start at zero.
                                behaviors: (0..keyboard.names.len() as u32)
                                    .map(|i| i + 10)
                                    .collect(),
                            },
                        )
                    }
                    behaviors::request::RequestType::GetBehaviorDetails(details) => {
                        behaviors::response::ResponseType::GetBehaviorDetails(
                            behaviors::GetBehaviorDetailsResponse {
                                id: details.behavior_id,
                                display_name: keyboard.names[(details.behavior_id - 10) as usize]
                                    .into(),
                            },
                        )
                    }
                };
                request_response::Subsystem::Behaviors(behaviors::Response {
                    response_type: Some(response),
                })
            }
            request::Subsystem::Keymap(keymap::Request { request_type }) => {
                let response = match request_type.unwrap() {
                    keymap::request::RequestType::GetKeymap(_) => {
                        keymap::response::ResponseType::GetKeymap(keyboard.live.clone())
                    }
                    keymap::request::RequestType::SetLayerBinding(set) => {
                        let known = set.binding.is_some_and(|b| b.behavior_id >= 10);
                        let slot = keyboard
                            .live
                            .layers
                            .iter_mut()
                            .find(|l| l.id == set.layer_id)
                            .and_then(|l| l.bindings.get_mut(set.key_position as usize));
                        let code = match (slot, known) {
                            (None, _) => keymap::SetLayerBindingResponse::InvalidLocation,
                            (_, false) => keymap::SetLayerBindingResponse::InvalidBehavior,
                            (Some(slot), true) => {
                                *slot = set.binding.unwrap();
                                keymap::SetLayerBindingResponse::Ok
                            }
                        };
                        keymap::response::ResponseType::SetLayerBinding(code as i32)
                    }
                    keymap::request::RequestType::CheckUnsavedChanges(_) => {
                        keymap::response::ResponseType::CheckUnsavedChanges(
                            keyboard.live != keyboard.saved,
                        )
                    }
                    keymap::request::RequestType::SaveChanges(_) => {
                        keyboard.saved = keyboard.live.clone();
                        keymap::response::ResponseType::SaveChanges(keymap::SaveChangesResponse {
                            result: Some(keymap::save_changes_response::Result::Ok(true)),
                        })
                    }
                    keymap::request::RequestType::DiscardChanges(_) => {
                        keyboard.live = keyboard.saved.clone();
                        keymap::response::ResponseType::DiscardChanges(true)
                    }
                };
                request_response::Subsystem::Keymap(keymap::Response {
                    response_type: Some(response),
                })
            }
        }
    }
}

impl Write for Wire {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let messages = self.decoder.borrow_mut().feed(bytes);
        for message in messages {
            let request = proto::Request::decode(message.as_slice()).unwrap();
            let request_id = request.request_id;
            // A notification arrives before the answer, as on real hardware.
            let notification = proto::Response {
                r#type: Some(response::Type::Notification(proto::Notification {
                    subsystem: Some(proto::notification::Subsystem::Keymap(
                        keymap::Notification {
                            notification_type: Some(
                                keymap::notification::NotificationType::UnsavedChangesStatusChanged(
                                    true,
                                ),
                            ),
                        },
                    )),
                })),
            };
            let answer = proto::Response {
                r#type: Some(response::Type::RequestResponse(proto::RequestResponse {
                    request_id,
                    subsystem: Some(self.answer(request)),
                })),
            };
            let mut outgoing = self.outgoing.borrow_mut();
            outgoing.extend(b"log noise");
            outgoing.extend(frame(&notification.encode_to_vec()));
            outgoing.extend(frame(&answer.encode_to_vec()));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Read for Wire {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let mut outgoing = self.outgoing.borrow_mut();
        // Trickle bytes out to exercise reassembly.
        let count = buffer.len().min(7).min(outgoing.len());
        for slot in buffer.iter_mut().take(count) {
            *slot = outgoing.pop_front().unwrap();
        }
        Ok(count)
    }
}

const NAMES: [&str; 9] = [
    "Key Press",
    "Transparent",
    "Momentary Layer",
    "Mod-Tap",
    "Layer-Tap",
    "Bluetooth",
    "Underglow",
    "Mouse Key Press",
    "magic",
];

fn go60() -> kc_boards::Board {
    kc_boards::built_in()
        .unwrap()
        .into_iter()
        .find(|b| b.id == "moergo-go60")
        .unwrap()
}

/// A keyboard running firmware built from `project`.
fn keyboard_running(project: &Project) -> (Wire, BehaviorTable) {
    let device: Vec<DeviceBehavior> = NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| DeviceBehavior {
            id: i as u32 + 10,
            display_name: (*name).into(),
        })
        .collect();
    let table = BehaviorTable::new(project, &device);
    let layers = project
        .layers
        .iter()
        .enumerate()
        .map(|(index, layer)| keymap::Layer {
            id: index as u32,
            name: layer.name.clone(),
            bindings: layer
                .bindings
                .iter()
                .map(|b| {
                    encode(project, &table, b).unwrap_or(keymap::BehaviorBinding {
                        behavior_id: 11,
                        param1: 0,
                        param2: 0,
                    })
                })
                .collect(),
        })
        .collect();
    let live = keymap::Keymap {
        layers,
        available_layers: 0,
        max_layer_name_length: 20,
    };
    let wire = Wire {
        keyboard: Rc::new(RefCell::new(Keyboard {
            unlocked: true,
            names: NAMES.to_vec(),
            saved: live.clone(),
            live,
        })),
        decoder: Rc::default(),
        outgoing: Rc::default(),
    };
    (wire, table)
}

fn kp(key: &str) -> Binding {
    Binding::kp(key.parse::<KeyExpr>().unwrap())
}

#[test]
fn keys_encode_as_zmk_does() {
    // ZMK: modifiers << 24 | usage page << 16 | usage.
    assert_eq!(encode_key(&"A".parse().unwrap()), Some(0x0007_0004));
    assert_eq!(encode_key(&"LC(LS(A))".parse().unwrap()), Some(0x0307_0004));
    assert_eq!(encode_key(&"EXCL".parse().unwrap()), Some(0x0207_001E));
    assert_eq!(encode_key(&"C_PP".parse().unwrap()), Some(0x000C_00CD));
    assert_eq!(encode_key(&"RG(K)".parse().unwrap()), Some(0x8007_000E));
    assert_eq!(encode_key(&KeyExpr::new("NOT_A_KEY")), None);

    for text in [
        "A",
        "LC(LS(A))",
        "EXCL",
        "C_PP",
        "RG(K)",
        "LSHFT",
        "N1",
        "LA(TAB)",
    ] {
        let expr: KeyExpr = text.parse().unwrap();
        let decoded = decode_key(encode_key(&expr).unwrap()).unwrap();
        assert_eq!(
            encode_key(&decoded),
            encode_key(&expr),
            "{text} -> {decoded}"
        );
    }
    // A shifted symbol comes back under its own name.
    assert_eq!(decode_key(0x0207_001E).unwrap().to_string(), "EXCL");
    assert_eq!(decode_key(0x0107_0004).unwrap().to_string(), "LC(A)");
}

#[test]
fn bindings_round_trip_through_the_protocol_numbers() {
    let project = Project::from_template("Go60", &go60());
    let (_, table) = keyboard_running(&project);
    let (base, magic) = (project.layers[0].id, project.layers[3].id);
    let magic_behavior = project
        .behaviors
        .iter()
        .find(|b| b.label == "magic")
        .unwrap()
        .id;
    let command = |label: &str, name: &str, args: &[u32]| {
        Binding::new(
            label,
            vec![Param::Command {
                name: name.into(),
                args: args.to_vec(),
            }],
        )
    };
    let cases = [
        (kp("LG(TAB)"), (10, 0x0807_002B, 0)),
        (Binding::trans(), (11, 0, 0)),
        (Binding::layer("mo", magic), (12, 3, 0)),
        (
            Binding::new(
                "mt",
                vec![
                    Param::Key(KeyExpr::new("RALT")),
                    Param::Key(KeyExpr::new("RET")),
                ],
            ),
            (13, 0x0007_00E6, 0x0007_0028),
        ),
        (
            Binding::new(
                "lt",
                vec![Param::Layer(magic), Param::Key(KeyExpr::new("SPACE"))],
            ),
            (14, 3, 0x0007_002C),
        ),
        (command("bt", "BT_SEL", &[2]), (15, 3, 2)),
        (command("bt", "BT_CLR", &[]), (15, 0, 0)),
        (command("rgb_ug", "RGB_BRI", &[]), (16, 7, 0)),
        (
            Binding::new("mkp", vec![Param::Constant("RCLK".into())]),
            (17, 2, 0),
        ),
        (
            Binding::user(magic_behavior, vec![Param::Layer(magic), Param::Number(0)]),
            (18, 3, 0),
        ),
    ];
    for (binding, (id, param1, param2)) in cases {
        let wire = encode(&project, &table, &binding).unwrap_or_else(|| panic!("{binding:?}"));
        assert_eq!(
            (wire.behavior_id, wire.param1, wire.param2),
            (id, param1, param2),
            "{binding:?}"
        );
        assert_eq!(decode(&project, &table, &wire), Some(binding));
    }
    let _ = base;
    // What the keyboard's firmware does not have cannot be sent.
    assert_eq!(
        encode(&project, &table, &Binding::new("bootloader", vec![])),
        None
    );
    assert_eq!(
        encode(&project, &table, &Binding::Raw { raw: "&x".into() }),
        None
    );
    // A colour takes three numbers, which the protocol has no room for.
    assert_eq!(
        encode(
            &project,
            &table,
            &command("rgb_ug", "RGB_COLOR_HSB", &[1, 2, 3])
        ),
        None
    );
}

#[test]
fn a_session_reads_compares_changes_and_saves() {
    let mut project = Project::from_template("Go60", &go60());
    let (wire, table) = keyboard_running(&project);
    let mut client = Client::new(wire.clone());

    assert_eq!(client.device_name().unwrap(), "Test Board");
    assert!(client.is_unlocked().unwrap());
    let behaviors = client.list_behaviors().unwrap();
    assert_eq!(behaviors.len(), NAMES.len());
    assert_eq!(
        (behaviors[0].id, behaviors[0].display_name.as_str()),
        (10, "Key Press")
    );

    // Fresh from a build, the keyboard matches the project.
    let device = client.get_keymap().unwrap();
    let same = compare(&project, &table, &device);
    assert!(same.changes.is_empty() && same.mismatch.is_none());
    // The simulated firmware has only a few behaviours, so keys using the
    // rest already count as needing a build.
    let baseline = same.needs_build;

    // Change three keys in the project; one cannot be sent.
    let (base, keypad) = (project.layers[0].id, project.layers[1].id);
    project.set_binding(base, 0, kp("LC(Z)")).unwrap();
    project
        .set_binding(keypad, 5, Binding::layer("mo", base))
        .unwrap();
    project
        .set_binding(base, 1, Binding::new("caps_word", vec![]))
        .unwrap();
    let difference = compare(&project, &table, &device);
    assert_eq!(difference.changes.len(), 2);
    assert_eq!(difference.needs_build, baseline + 1);
    assert_eq!(
        (
            difference.changes[0].layer_id,
            difference.changes[0].position
        ),
        (0, 0)
    );
    assert_eq!(
        (
            difference.changes[1].layer_id,
            difference.changes[1].position
        ),
        (1, 5)
    );

    for change in &difference.changes {
        client
            .set_binding(change.layer_id, change.position, change.binding)
            .unwrap();
    }
    assert!(client.has_unsaved_changes().unwrap());
    let after = client.get_keymap().unwrap();
    assert!(compare(&project, &table, &after).changes.is_empty());
    assert_eq!(
        decode(&project, &table, &after.layers[0].bindings[0]),
        Some(kp("LC(Z)"))
    );

    // Discarding goes back to what was saved; saving keeps it.
    client.discard().unwrap();
    assert!(!client.has_unsaved_changes().unwrap());
    assert_eq!(
        compare(&project, &table, &client.get_keymap().unwrap())
            .changes
            .len(),
        2
    );
    for change in &difference.changes {
        client
            .set_binding(change.layer_id, change.position, change.binding)
            .unwrap();
    }
    client.save().unwrap();
    assert!(!client.has_unsaved_changes().unwrap());
    assert_eq!(
        wire.keyboard.borrow().saved.layers[1].bindings[5].behavior_id,
        12
    );
}

#[test]
fn refusals_and_locks_are_reported() {
    let project = Project::from_template("Go60", &go60());
    let (wire, table) = keyboard_running(&project);
    let mut client = Client::new(wire.clone());
    let binding = encode(&project, &table, &kp("A")).unwrap();

    assert!(matches!(
        client.set_binding(0, 999, binding),
        Err(StudioError::Refused {
            position: 999,
            reason: "no such key"
        })
    ));
    let unknown = keymap::BehaviorBinding {
        behavior_id: 1,
        ..binding
    };
    assert!(matches!(
        client.set_binding(0, 0, unknown),
        Err(StudioError::Refused {
            reason: "unknown behaviour",
            ..
        })
    ));

    wire.keyboard.borrow_mut().unlocked = false;
    // The name and lock state can be read while locked; the keymap cannot.
    assert_eq!(client.device_name().unwrap(), "Test Board");
    assert!(!client.is_unlocked().unwrap());
    assert!(matches!(client.get_keymap(), Err(StudioError::Locked)));
    assert!(matches!(
        client.set_binding(0, 0, binding),
        Err(StudioError::Locked)
    ));
}

#[test]
fn a_keyboard_running_something_else_is_noticed() {
    let project = Project::from_template("Go60", &go60());
    let (wire, table) = keyboard_running(&project);
    let mut client = Client::new(wire.clone());

    let mut fewer = client.get_keymap().unwrap();
    fewer.layers.pop();
    assert!(compare(&project, &table, &fewer)
        .mismatch
        .unwrap()
        .contains("4 layers"));

    let mut rearranged = client.get_keymap().unwrap();
    rearranged.layers[4].id = 9;
    assert!(compare(&project, &table, &rearranged)
        .mismatch
        .unwrap()
        .contains("rearranged"));

    let mut smaller = client.get_keymap().unwrap();
    smaller.layers[0].bindings.pop();
    assert!(compare(&project, &table, &smaller)
        .mismatch
        .unwrap()
        .contains("different number of keys"));

    // A stream that goes quiet is not waited on forever.
    struct Silent;
    impl Read for Silent {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Ok(0)
        }
    }
    impl Write for Silent {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert!(matches!(
        Client::new(Silent).device_name(),
        Err(StudioError::NoAnswer)
    ));
}
