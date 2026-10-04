//! Debug aid: where a round trip through the importer changes a keymap.
fn main() {
    let boards = kc_boards::built_in().unwrap();
    let path = std::env::args().nth(1).unwrap();
    let project = kc_model::file::load(path.as_ref()).unwrap();
    let board = boards.iter().find(|b| b.id == project.board).unwrap();
    let original = kc_emit::keymap(&project, board).unwrap();
    let (mut imported, report) = kc_import::import_keymap("x", &original, board).unwrap();
    imported.firmware = project.firmware.clone();
    let again = kc_emit::keymap(&imported, board).unwrap();
    println!("{report:?}");
    for (a, b) in original.lines().zip(again.lines()) {
        if a != b {
            println!("- {a}\n+ {b}");
        }
    }
}
