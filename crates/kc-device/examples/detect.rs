//! Lists the connected USB devices that are, or may be, a board the app
//! knows. It only looks: nothing is opened or written.
//!
//! `cargo run -p kc-device --example detect`

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let boards = kc_boards::built_in()?;
    let found = kc_device::detect(&boards);
    if found.is_empty() {
        println!("No supported keyboard found on USB.");
    }
    for device in found {
        let names: Vec<&str> = device
            .boards
            .iter()
            .map(|i| boards[*i].name.as_str())
            .collect();
        println!(
            "{} ({:04x}:{:04x}): {} {}; {}",
            device.usb.label(),
            device.usb.vendor,
            device.usb.product,
            if device.certain { "is a" } else { "may be a" },
            names.join(" or "),
            match device.usb.link() {
                Some(link) => format!("can be linked, serial {}", link.serial),
                None => "reports no serial number, so cannot be linked".to_string(),
            }
        );
    }
    Ok(())
}
