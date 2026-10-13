//! VIA over the keyboard's raw HID interface.
//!
//! QMK puts VIA on an interface of its own, usage page `0xFF60`, usage
//! `0x61`, and moves one 32-byte report each way per command: the host
//! sends a request, the keyboard answers with the same buffer filled in.

use std::time::Duration;

use async_hid::{AsyncHidRead, AsyncHidWrite, Device, DeviceReader, DeviceWriter, HidBackend};
use futures_util::StreamExt;

use crate::{Transport, REPORT};

pub const VIA_USAGE_PAGE: u16 = 0xFF60;
pub const VIA_USAGE: u16 = 0x61;
/// How long the keyboard may take to answer one report.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(2);

/// A keyboard's VIA interface, found on USB.
pub struct Found {
    device: Device,
    pub name: String,
    pub vendor: u16,
    pub product: u16,
    pub serial: Option<String>,
}

impl Found {
    /// Every VIA interface on USB, narrowed to one keyboard's identifiers
    /// when given, and to one serial number when given.
    pub fn all(ids: Option<(u16, u16)>, serial: Option<&str>) -> std::io::Result<Vec<Found>> {
        let runtime = runtime()?;
        runtime.block_on(async {
            let backend = HidBackend::default();
            let mut devices = backend.enumerate().await.map_err(std::io::Error::other)?;
            let mut found = Vec::new();
            while let Some(device) = devices.next().await {
                if device.usage_page != VIA_USAGE_PAGE || device.usage_id != VIA_USAGE {
                    continue;
                }
                if ids.is_some_and(|(v, p)| v != device.vendor_id || p != device.product_id) {
                    continue;
                }
                if serial.is_some_and(|s| device.serial_number.as_deref().map(str::trim) != Some(s))
                {
                    continue;
                }
                found.push(Found {
                    name: device.name.clone(),
                    vendor: device.vendor_id,
                    product: device.product_id,
                    serial: device.serial_number.clone(),
                    device,
                });
            }
            found.sort_by(|a, b| {
                (a.vendor, a.product, &a.serial).cmp(&(b.vendor, b.product, &b.serial))
            });
            Ok(found)
        })
    }

    /// A name to show for the keyboard.
    pub fn label(&self) -> String {
        if self.name.trim().is_empty() {
            format!("USB {:04x}:{:04x}", self.vendor, self.product)
        } else {
            self.name.clone()
        }
    }

    /// Opens the interface for a conversation.
    pub fn open(self) -> std::io::Result<HidTransport> {
        let runtime = runtime()?;
        let (reader, writer) = runtime
            .block_on(self.device.open())
            .map_err(std::io::Error::other)?;
        Ok(HidTransport {
            runtime,
            reader,
            writer,
        })
    }
}

fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}

/// One open VIA interface. Each request is one output report and its
/// answer one input report.
pub struct HidTransport {
    runtime: tokio::runtime::Runtime,
    reader: DeviceReader,
    writer: DeviceWriter,
}

impl Transport for HidTransport {
    fn exchange(&mut self, request: &[u8; REPORT]) -> std::io::Result<[u8; REPORT]> {
        self.runtime.block_on(async {
            // Byte zero is the report ID, always zero.
            let mut out = [0u8; REPORT + 1];
            out[1..].copy_from_slice(request);
            self.writer
                .write_output_report(&out)
                .await
                .map_err(std::io::Error::other)?;
            let mut answer = [0u8; REPORT];
            let read =
                tokio::time::timeout(ANSWER_TIMEOUT, self.reader.read_input_report(&mut answer))
                    .await
                    .map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "the keyboard did not answer",
                        )
                    })?
                    .map_err(std::io::Error::other)?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "the keyboard closed the connection",
                ));
            }
            Ok(answer)
        })
    }
}
