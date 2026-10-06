//! The USB serial connection ZMK Studio uses.

use std::time::Duration;

use kc_model::Device;
use serialport::{SerialPort, SerialPortType};

/// A USB serial port, and the device it belongs to when the device
/// reports a serial number.
#[derive(Debug, Clone, PartialEq)]
pub struct Port {
    pub name: String,
    pub device: Option<Device>,
}

/// The serial ports that could be a keyboard: USB ones. On macOS each
/// device shows up twice; the `cu.` form is the one to open.
pub fn candidate_ports() -> Vec<Port> {
    let mut ports: Vec<Port> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|port| match port.port_type {
            SerialPortType::UsbPort(usb) => Some(Port {
                name: port.port_name,
                device: usb
                    .serial_number
                    .map(|serial| serial.trim().to_string())
                    .filter(|serial| !serial.is_empty())
                    .map(|serial| Device {
                        vendor: usb.vid,
                        product: usb.pid,
                        serial,
                    }),
            }),
            _ => None,
        })
        .filter(|port| !port.name.contains("/tty."))
        .collect();
    ports.sort_by(|a, b| a.name.cmp(&b.name));
    ports
}

/// Opens a port for the Studio protocol.
pub fn open(port: &str) -> Result<Box<dyn SerialPort>, serialport::Error> {
    // The speed is ignored by a USB serial device, but one must be given.
    serialport::new(port, 115_200)
        .timeout(Duration::from_millis(1500))
        .open()
}
