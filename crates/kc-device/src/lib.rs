//! Finds keyboards connected by USB and works out which board each one is.
//!
//! A board definition lists how its stock firmware identifies itself. The
//! vendor and product IDs are often shared between boards, and the product
//! name follows the keyboard name the user can change, so a match is
//! either certain (IDs and name) or only possible (IDs alone), and the
//! user confirms the latter.

use kc_boards::Board;
use kc_model::Device;
use nusb::MaybeFuture as _;

/// A USB device, as it describes itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbDevice {
    pub vendor: u16,
    pub product: u16,
    pub name: Option<String>,
    pub serial: Option<String>,
}

impl UsbDevice {
    /// What a saved keyboard is linked to. `None` when the device reports
    /// no serial number, since nothing then tells it from its twins.
    pub fn link(&self) -> Option<Device> {
        let serial = self.serial.as_deref()?.trim();
        if serial.is_empty() {
            return None;
        }
        Some(Device {
            vendor: self.vendor,
            product: self.product,
            serial: serial.to_string(),
        })
    }

    /// A name to show for the device.
    pub fn label(&self) -> String {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map_or_else(
                || format!("USB device {:04x}:{:04x}", self.vendor, self.product),
                str::to_string,
            )
    }
}

/// A connected device that is, or may be, a board the app knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    pub usb: UsbDevice,
    /// The boards it could be, as indices into the board list. Never empty.
    pub boards: Vec<usize>,
    /// Whether the device's name settles which board it is. Otherwise only
    /// its IDs matched, and the user should confirm the model.
    pub certain: bool,
}

/// Picks out the devices that could be one of `boards`.
pub fn identify(devices: &[UsbDevice], boards: &[Board]) -> Vec<Detected> {
    devices
        .iter()
        .filter_map(|device| {
            let name = device.name.as_deref().map(str::trim);
            let mut by_name = Vec::new();
            let mut by_ids = Vec::new();
            for (index, board) in boards.iter().enumerate() {
                let ids = board
                    .usb
                    .iter()
                    .filter(|id| id.vendor == device.vendor && id.product == device.product);
                let mut matched = false;
                let mut named = false;
                for id in ids {
                    matched = true;
                    named |= name.is_some_and(|name| name.eq_ignore_ascii_case(&id.name));
                }
                if named {
                    by_name.push(index);
                } else if matched {
                    by_ids.push(index);
                }
            }
            let certain = by_name.len() == 1;
            let boards = if by_name.is_empty() { by_ids } else { by_name };
            (!boards.is_empty()).then(|| Detected {
                usb: device.clone(),
                boards,
                certain,
            })
        })
        .collect()
}

/// Every USB device connected right now. Failing to list them reads as
/// none being connected.
pub fn connected() -> Vec<UsbDevice> {
    let Ok(devices) = nusb::list_devices().wait() else {
        return Vec::new();
    };
    devices
        .map(|info| UsbDevice {
            vendor: info.vendor_id(),
            product: info.product_id(),
            name: info.product_string().map(str::to_string),
            serial: info.serial_number().map(str::to_string),
        })
        .collect()
}

/// The connected devices that could be one of `boards`.
pub fn detect(boards: &[Board]) -> Vec<Detected> {
    identify(&connected(), boards)
}

/// Whether a linked device is connected right now.
pub fn is_connected(device: &Device, connected: &[UsbDevice]) -> bool {
    connected
        .iter()
        .any(|usb| usb.link().as_ref() == Some(device))
}
