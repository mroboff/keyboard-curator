//! Rynk over the keyboard's vendor HID interface.
//!
//! Firmware built with RMK's `rynk` feature puts the protocol on an
//! interface of its own, usage page `0xFF14`, usage `0x61`, and moves
//! frames in fixed 32-byte reports with zero padding. The framing here is
//! Rynk's own, as its `rynk-usb` transport does it (MIT OR Apache-2.0,
//! rmk-rs/rmk); this one also knows which keyboard it is looking for.

use async_hid::{AsyncHidRead, AsyncHidWrite, Device, DeviceReader, DeviceWriter, HidBackend};
use futures_util::StreamExt;
use kc_boards::Board;
use kc_model::Device as Link;
use rynk::io::{ErrorType, Read, Write};
use rynk::rmk_types::protocol::rynk::{RYNK_HID_REPORT_SIZE, RYNK_MAGIC};
use rynk::{RynkDevice, RynkHostError};

const RYNK_USAGE_PAGE: u16 = 0xFF14;
const RYNK_USAGE: u16 = 0x61;

/// A keyboard's Rynk interface, found on USB.
pub struct HidDevice {
    device: Device,
    pub name: String,
    pub vendor: u16,
    pub product: u16,
    pub serial: Option<String>,
}

/// A USB serial number as the board reports it under its stock firmware:
/// Rynk firmware prefixes its own with `rynk:`, which is dropped.
pub fn bare_serial(serial: &str) -> &str {
    serial
        .trim()
        .strip_prefix(RYNK_MAGIC)
        .unwrap_or(serial.trim())
}

/// Every Rynk interface of a keyboard of `board`, the one linked as
/// `link` first if it is there.
pub async fn discover(board: &Board, link: Option<&Link>) -> Result<Vec<HidDevice>, RynkHostError> {
    let backend = HidBackend::default();
    let mut devices = backend
        .enumerate()
        .await
        .map_err(|e| RynkHostError::Transport("enumerate_hid", e.to_string()))?;
    let mut found = Vec::new();
    while let Some(device) = devices.next().await {
        if device.usage_page != RYNK_USAGE_PAGE || device.usage_id != RYNK_USAGE {
            continue;
        }
        let ours = board
            .usb
            .iter()
            .any(|id| id.vendor == device.vendor_id && id.product == device.product_id);
        if !ours {
            continue;
        }
        found.push(HidDevice {
            name: device.name.clone(),
            vendor: device.vendor_id,
            product: device.product_id,
            serial: device.serial_number.clone(),
            device,
        });
    }
    if let Some(link) = link {
        found.sort_by_key(|d| !d.is_linked(link));
    }
    Ok(found)
}

impl HidDevice {
    /// Whether this is the keyboard a saved board is linked to.
    pub fn is_linked(&self, link: &Link) -> bool {
        self.vendor == link.vendor
            && self.product == link.product
            && self
                .serial
                .as_deref()
                .is_some_and(|serial| bare_serial(serial) == bare_serial(&link.serial))
    }
}

impl RynkDevice for HidDevice {
    type Read = HidReader;
    type Write = HidWriter;

    fn label(&self) -> String {
        if self.name.is_empty() {
            format!("USB {:04x}:{:04x}", self.vendor, self.product)
        } else {
            self.name.clone()
        }
    }

    async fn open(self) -> Result<(HidReader, HidWriter), RynkHostError> {
        let (reader, writer) = self
            .device
            .open()
            .await
            .map_err(|e| RynkHostError::Transport("open_hid", e.to_string()))?;
        Ok((
            HidReader {
                reader,
                report: [0; RYNK_HID_REPORT_SIZE],
                pos: 0,
                end: 0,
            },
            HidWriter {
                writer,
                pending: [0; RYNK_HID_REPORT_SIZE + 1],
                len: 0,
            },
        ))
    }
}

/// Reads whole reports and hands them out in whatever pieces the driver
/// asks for. Report padding is zero bytes, which the driver's deframer
/// reads as frame delimiters.
pub struct HidReader {
    reader: DeviceReader,
    report: [u8; RYNK_HID_REPORT_SIZE],
    pos: usize,
    end: usize,
}

impl ErrorType for HidReader {
    type Error = std::io::Error;
}

impl Read for HidReader {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.pos == self.end {
            self.end = self
                .reader
                .read_input_report(&mut self.report)
                .await
                .map_err(std::io::Error::other)?;
            self.pos = 0;
            if self.end == 0 {
                return Ok(0);
            }
        }
        let n = buf.len().min(self.end - self.pos);
        buf[..n].copy_from_slice(&self.report[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Packs frame bytes into fixed-size reports: a report goes out when it is
/// full or when the frame ends. Byte zero is the report ID, always zero.
pub struct HidWriter {
    writer: DeviceWriter,
    pending: [u8; RYNK_HID_REPORT_SIZE + 1],
    len: usize,
}

impl ErrorType for HidWriter {
    type Error = std::io::Error;
}

impl Write for HidWriter {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        let remaining = RYNK_HID_REPORT_SIZE - self.len;
        let n = buf
            .iter()
            .position(|byte| *byte == 0)
            .map_or(buf.len(), |index| index + 1)
            .min(remaining);
        self.pending[1 + self.len..1 + self.len + n].copy_from_slice(&buf[..n]);
        self.len += n;
        if self.len == RYNK_HID_REPORT_SIZE || buf[n - 1] == 0 {
            self.writer
                .write_output_report(&self.pending)
                .await
                .map_err(std::io::Error::other)?;
            self.pending.fill(0);
            self.len = 0;
        }
        Ok(n)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        if self.len != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "incomplete Rynk frame",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rynk_firmware_marks_its_serial_and_the_mark_is_dropped() {
        assert_eq!(bare_serial("rynk:ABC123"), "ABC123");
        assert_eq!(bare_serial(" ABC123 "), "ABC123");
        assert_eq!(RYNK_MAGIC, "rynk:");
    }
}
