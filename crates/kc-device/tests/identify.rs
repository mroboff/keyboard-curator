//! Recognizing boards from what USB devices report.

use kc_device::{identify, is_connected, UsbDevice};

fn usb(vendor: u16, product: u16, name: Option<&str>, serial: Option<&str>) -> UsbDevice {
    UsbDevice {
        vendor,
        product,
        name: name.map(str::to_string),
        serial: serial.map(str::to_string),
    }
}

#[test]
fn boards_are_recognized_by_ids_and_name() {
    let boards = kc_boards::built_in().unwrap();
    let index = |id: &str| boards.iter().position(|b| b.id == id).unwrap();
    let devices = [
        usb(0x046d, 0x085e, Some("Logitech BRIO"), Some("2220BC50")),
        usb(
            0x16c0,
            0x27db,
            Some("Go60 Left"),
            Some("moergo.com:GO60-1A2B"),
        ),
        usb(0x1d50, 0x615e, Some("imprint"), Some("ABCDEF")),
        // A renamed Imprint, or any other ZMK keyboard with default IDs.
        usb(0x1d50, 0x615e, Some("Mark's Board"), None),
        // MoErgo's IDs are shared by its models; the name tells them apart.
        usb(
            0x16c0,
            0x27db,
            Some("Glove80 Left"),
            Some("moergo.com:GLV80-9"),
        ),
        // MoErgo's IDs with a name no board has.
        usb(
            0x16c0,
            0x27db,
            Some("Glove100 Left"),
            Some("moergo.com:GLV100-9"),
        ),
    ];

    let found = identify(&devices, &boards);
    assert_eq!(found.len(), 5);

    assert_eq!(found[0].usb, devices[1]);
    assert_eq!(found[0].boards, [index("moergo-go60")]);
    assert!(found[0].certain);

    // Names are compared without regard to case.
    assert_eq!(found[1].boards, [index("cyboard-imprint")]);
    assert!(found[1].certain);

    assert_eq!(found[2].boards, [index("cyboard-imprint")]);
    assert!(!found[2].certain);

    assert_eq!(found[3].boards, [index("moergo-glove80")]);
    assert!(found[3].certain);

    let mut moergo = found[4].boards.clone();
    moergo.sort_unstable();
    let mut expected = [index("moergo-go60"), index("moergo-glove80")];
    expected.sort_unstable();
    assert_eq!(moergo, expected);
    assert!(!found[4].certain);
}

#[test]
fn only_devices_with_a_serial_number_can_be_linked() {
    let go60 = usb(
        0x16c0,
        0x27db,
        Some("Go60 Left"),
        Some(" moergo.com:GO60-1A2B "),
    );
    let link = go60.link().unwrap();
    assert_eq!(link.serial, "moergo.com:GO60-1A2B");
    // RMK firmware marks its serial number; the keyboard stays the same
    // device.
    let marked = usb(
        0x16c0,
        0x27db,
        Some("Go60"),
        Some("rynk:moergo.com:GO60-1A2B"),
    );
    assert_eq!(marked.link().unwrap(), link);
    assert_eq!((link.vendor, link.product), (0x16c0, 0x27db));

    assert!(usb(1, 2, Some("Nameless"), None).link().is_none());
    assert!(usb(1, 2, Some("Blank"), Some("  ")).link().is_none());

    let other = usb(
        0x16c0,
        0x27db,
        Some("Go60 Left"),
        Some("moergo.com:GO60-FFFF"),
    );
    assert!(is_connected(&link, &[other.clone(), go60.clone()]));
    assert!(!is_connected(&link, &[other]));
}

#[test]
fn devices_are_labeled_by_name_or_ids() {
    assert_eq!(usb(1, 2, Some(" Go60 Left "), None).label(), "Go60 Left");
    assert_eq!(
        usb(0x16c0, 0x27db, None, None).label(),
        "USB device 16c0:27db"
    );
    assert_eq!(
        usb(0x16c0, 0x27db, Some(""), None).label(),
        "USB device 16c0:27db"
    );
}

#[test]
fn listing_connected_devices_does_not_fail() {
    // Whatever is plugged in, listing and matching must not panic.
    let boards = kc_boards::built_in().unwrap();
    let _ = kc_device::detect(&boards);
}
