//! The USB serial connection ZMK Studio uses.

use std::time::Duration;

use serialport::{SerialPort, SerialPortType};

/// The serial ports that could be a keyboard: USB ones. On macOS each
/// device shows up twice; the `cu.` form is the one to open.
pub fn candidate_ports() -> Vec<String> {
    let mut ports: Vec<String> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter(|port| matches!(port.port_type, SerialPortType::UsbPort(_)))
        .map(|port| port.port_name)
        .filter(|name| !name.contains("/tty."))
        .collect();
    ports.sort();
    ports
}

/// Opens a port for the Studio protocol.
pub fn open(port: &str) -> Result<Box<dyn SerialPort>, serialport::Error> {
    // The speed is ignored by a USB serial device, but one must be given.
    serialport::new(port, 115_200)
        .timeout(Duration::from_millis(1500))
        .open()
}
