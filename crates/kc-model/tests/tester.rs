//! The key tester: matching what the computer sees to keys of a layout.

use kc_boards::Board;
use kc_model::tester::{grid, positions_sending, testable, Tester};
use kc_model::{Binding, KeyExpr, Param, Project};
use kc_zmk::Modifier;

fn board(id: &str) -> Board {
    kc_boards::built_in()
        .unwrap()
        .into_iter()
        .find(|b| b.id == id)
        .unwrap()
}

fn key(name: &str) -> Param {
    Param::Key(KeyExpr::new(name))
}

/// A Go60 layout with nothing but what each test puts on it.
fn blank() -> (Project, kc_model::LayerId, kc_model::LayerId) {
    let go60 = board("moergo-go60");
    let mut project = Project::new("Test", &go60);
    let base = project.layers[0].id;
    for position in 0..project.key_count {
        project
            .set_binding(base, position, Binding::trans())
            .unwrap();
    }
    let nav = project.add_layer("Nav").unwrap();
    (project, base, nav)
}

#[test]
fn a_press_is_matched_to_every_key_that_can_send_it() {
    let (mut project, base, nav) = blank();
    project
        .set_binding(base, 0, Binding::kp(KeyExpr::new("A")))
        .unwrap();
    // The same key by another of its names, on another layer.
    project
        .set_binding(nav, 5, Binding::kp(KeyExpr::new("A")))
        .unwrap();
    project
        .set_binding(base, 1, Binding::kp(KeyExpr::new("RET")))
        .unwrap();
    project
        .set_binding(nav, 2, Binding::kp(KeyExpr::new("ENTER")))
        .unwrap();

    assert_eq!(positions_sending(&project, "A"), [0, 5]);
    // Names are compared by the key they mean, not by spelling.
    assert_eq!(positions_sending(&project, "RET"), [1, 2]);
    assert_eq!(positions_sending(&project, "ENTER"), [1, 2]);
    assert!(positions_sending(&project, "B").is_empty());
    assert!(positions_sending(&project, "NOT_A_KEY").is_empty());
}

#[test]
fn keys_that_do_two_things_match_both() {
    let (mut project, base, nav) = blank();
    // Shift when held, A when tapped.
    project
        .set_binding(base, 0, Binding::new("mt", vec![key("LSHFT"), key("A")]))
        .unwrap();
    // A layer when held, Space when tapped.
    project
        .set_binding(
            base,
            1,
            Binding::new("lt", vec![Param::Layer(nav), key("SPACE")]),
        )
        .unwrap();
    // Ctrl-C, and a key whose name means a shifted key.
    project
        .set_binding(
            base,
            2,
            Binding::kp(KeyExpr::new("C").with(Modifier::LCtrl)),
        )
        .unwrap();
    project
        .set_binding(base, 3, Binding::kp(KeyExpr::new("EXCL")))
        .unwrap();

    assert_eq!(positions_sending(&project, "A"), [0]);
    assert_eq!(positions_sending(&project, "SPACE"), [1]);
    assert_eq!(positions_sending(&project, "C"), [2]);
    assert_eq!(positions_sending(&project, "LCTRL"), [2]);
    assert_eq!(positions_sending(&project, "N1"), [3]);
    // The mod-tap's shift, and the shift the exclamation mark needs.
    assert_eq!(positions_sending(&project, "LSHFT"), [0, 3]);
}

#[test]
fn keys_that_send_nothing_to_the_computer_cannot_be_tested() {
    let (mut project, base, nav) = blank();
    project
        .set_binding(base, 0, Binding::kp(KeyExpr::new("A")))
        .unwrap();
    project
        .set_binding(base, 1, Binding::layer("mo", nav))
        .unwrap();
    project
        .set_binding(base, 2, Binding::new("bootloader", vec![]))
        .unwrap();
    // See-through on the base layer, a key on another.
    project
        .set_binding(nav, 3, Binding::kp(KeyExpr::new("F5")))
        .unwrap();
    project
        .set_binding(
            base,
            4,
            Binding::Raw {
                raw: "&custom".into(),
            },
        )
        .unwrap();

    let can = testable(&project);
    assert_eq!(can.len(), project.key_count);
    assert_eq!(can[..5], [true, false, false, true, false]);
    assert_eq!(can.iter().filter(|t| **t).count(), 2);
}

#[test]
fn the_tester_tracks_what_is_down_and_what_has_been_seen() {
    let can = [true, true, false, true];
    let mut tester = Tester::default();
    assert_eq!(tester.progress(&can), (0, 3));

    tester.press(&[0, 1]);
    assert!(tester.is_pressed(0) && tester.is_pressed(1));
    tester.release(&[0]);
    assert!(!tester.is_pressed(0) && tester.is_seen(0));
    assert!(tester.is_pressed(1));
    assert_eq!(tester.progress(&can), (2, 3));

    // A key that cannot be tested does not count, even if marked.
    tester.press(&[2]);
    assert_eq!(tester.progress(&can), (2, 3));
    tester.press(&[3]);
    assert_eq!(tester.progress(&can), (3, 3));

    tester.release_all();
    assert!(!tester.is_pressed(1) && tester.is_seen(1));
    tester.reset();
    assert_eq!(tester, Tester::default());
}

#[test]
fn keys_get_a_column_and_a_row_from_where_they_sit() {
    for id in ["moergo-go60", "cyboard-imprint", "dygma-defy"] {
        let board = board(id);
        let keys = &board.layout(&board.default_layout).unwrap().keys;
        let cells = grid(keys);
        assert_eq!(cells.len(), keys.len(), "{id}");
        // Columns run from the left edge and rows from the bottom.
        assert_eq!(cells.iter().map(|c| c.0).min(), Some(0), "{id}");
        assert_eq!(cells.iter().map(|c| c.1).min(), Some(0), "{id}");
        assert!(cells.iter().all(|c| c.0 >= 0 && c.1 >= 0), "{id}");
        // A board is wider than it is tall, and has several of each.
        let columns = cells.iter().map(|c| c.0).max().unwrap();
        let rows = cells.iter().map(|c| c.1).max().unwrap();
        assert!(columns > rows && rows >= 3, "{id}: {columns} x {rows}");
    }
    // On the Go60 the first two keys are neighbors on the top row.
    let go60 = board("moergo-go60");
    let cells = grid(&go60.layout(&go60.default_layout).unwrap().keys);
    assert_eq!(cells[1].0 - cells[0].0, 1);
    assert!(cells[0].1 >= 3);
}
