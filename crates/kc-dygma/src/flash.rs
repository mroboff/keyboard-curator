//! Installing firmware on the Neuron of a wireless Defy.
//!
//! The Neuron 2 is an nRF52833 with Dygma's bootloader. Flashing it is
//! what Bazecor's firmware update does, as read from Bazecor's source
//! (`src/api/flash`, GPL-3.0; facts only, no code copied):
//!
//! 1. Over Focus, `upgrade.start` makes the Neuron release every key and
//!    check its sides, and `upgrade.neuron` restarts it into the
//!    bootloader, which appears as a new USB serial device (product
//!    [`BOOTLOADER_PRODUCT`]) with the Neuron's light pulsing blue.
//! 2. The bootloader speaks a short text protocol at 115200 baud: `E`
//!    erases from an address, `U` announces a packet's size, the packet's
//!    bytes follow, `W` writes them at an address, and `S` starts the
//!    application. Every number is eight hexadecimal digits and every
//!    command ends with `#`. The bootloader answers each command with a
//!    byte (`A`), and sends an information block of more than forty bytes
//!    which Bazecor reads and ignores.
//! 3. The firmware is an Intel HEX file; its data records, with their
//!    extended addresses applied, are written in order from the first
//!    record's address in packets of up to [`PACKET`] bytes.
//!
//! The bootloader itself is never written, so a flash that fails part-way
//! leaves the Neuron in the bootloader, where a second attempt, or
//! Bazecor's firmware update, flashes it again. Everything here is checked
//! against a simulated bootloader; see the tests.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use crate::{Focus, FocusError};

/// The USB vendor of every Dygma keyboard.
pub const VENDOR: u16 = 0x35ef;
/// The wireless Defy's Neuron while running its firmware.
pub const KEYBOARD_PRODUCT: u16 = 0x0012;
/// The wireless Defy's Neuron in its bootloader.
pub const BOOTLOADER_PRODUCT: u16 = 0x0013;

/// The most bytes sent in one `U`/`W` packet.
pub const PACKET: usize = 4096;
/// Where the Neuron's application area ends: Dygma's bootloader keeps the
/// Bluetooth pairings from here and the emulated EEPROM, then itself,
/// above that. Nothing is ever written at or past this address.
pub const APPLICATION_END: u32 = 0x0007_0000;
/// Where the wireless Defy's firmware image begins: the RF host image at
/// `0x27000`, followed by the application at `0x2E000`. An image starting
/// anywhere else is not a Neuron image.
pub const IMAGE_STARTS: [u32; 2] = [0x0002_7000, 0x0002_E000];

#[derive(Debug, thiserror::Error)]
pub enum FlashError {
    #[error("the firmware file is not an Intel HEX file: {0}")]
    Hex(String),
    #[error("the firmware image starts at {start:#x}, which is not where a Neuron image starts ({starts})")]
    WrongStart { start: u32, starts: String },
    #[error("the firmware image ends at {end:#x}, past the Neuron's application area ({limit:#x}); nothing was written")]
    TooLarge { end: u32, limit: u32 },
    #[error("the Neuron's bootloader did not answer `{0}` in time")]
    NoAnswer(String),
    #[error("the Neuron did not appear in its bootloader within {0} seconds. If its light is not pulsing blue, unplug it, plug it back in and try again; Bazecor's firmware update is the other way to install firmware")]
    NoBootloader(u64),
    #[error("the keyboard did not come back after flashing within {0} seconds. Unplug it and plug it back in; if it stays in its bootloader (light pulsing blue), run the install again or use Bazecor's firmware update")]
    NoKeyboard(u64),
    #[error("{0}")]
    Focus(#[from] FocusError),
    #[error("could not talk to the bootloader: {0}")]
    Io(#[from] std::io::Error),
}

/// A firmware image as one contiguous run of bytes at an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub start: u32,
    pub bytes: Vec<u8>,
}

impl Image {
    /// The first address past the image.
    pub fn end(&self) -> u32 {
        self.start + u32::try_from(self.bytes.len()).unwrap_or(u32::MAX)
    }

    /// Checks that the image is one a Neuron takes.
    pub fn check(&self) -> Result<(), FlashError> {
        if !IMAGE_STARTS.contains(&self.start) {
            return Err(FlashError::WrongStart {
                start: self.start,
                starts: IMAGE_STARTS
                    .iter()
                    .map(|s| format!("{s:#x}"))
                    .collect::<Vec<_>>()
                    .join(" or "),
            });
        }
        if self.end() > APPLICATION_END {
            return Err(FlashError::TooLarge {
                end: self.end(),
                limit: APPLICATION_END,
            });
        }
        Ok(())
    }
}

/// Reads an Intel HEX file into one image: every data record at its full
/// address, with gaps between records filled with `0xFF` (erased flash),
/// which is what the bootloader writes in order from the first address.
pub fn parse_hex(text: &str) -> Result<Image, FlashError> {
    let bad = |line: usize, what: &str| FlashError::Hex(format!("line {}: {what}", line + 1));
    let mut records: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut base: u32 = 0;
    for (number, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let body = line
            .strip_prefix(':')
            .ok_or_else(|| bad(number, "a record must start with `:`"))?;
        if body.len() % 2 != 0 || body.len() < 10 {
            return Err(bad(number, "a record is an even number of hex digits"));
        }
        let bytes: Vec<u8> = (0..body.len() / 2)
            .map(|i| u8::from_str_radix(&body[2 * i..2 * i + 2], 16))
            .collect::<Result<_, _>>()
            .map_err(|_| bad(number, "not hexadecimal"))?;
        let count = usize::from(bytes[0]);
        if bytes.len() != count + 5 {
            return Err(bad(number, "the record's length does not match its count"));
        }
        let sum: u8 = bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b));
        if sum != 0 {
            return Err(bad(number, "the record's checksum is wrong"));
        }
        let offset = u32::from(u16::from_be_bytes([bytes[1], bytes[2]]));
        let kind = bytes[3];
        let data = &bytes[4..4 + count];
        match kind {
            0x00 => records.push((base + offset, data.to_vec())),
            0x01 => break,
            0x02 => {
                let segment = u32::from(u16::from_be_bytes([data[0], data[1]]));
                base = segment << 4;
            }
            0x04 => {
                let upper = u32::from(u16::from_be_bytes([data[0], data[1]]));
                base = upper << 16;
            }
            0x03 | 0x05 => {}
            other => {
                return Err(bad(
                    number,
                    &format!("record type {other:#04x} is not known"),
                ))
            }
        }
    }
    let (Some(start), Some(end)) = (
        records.iter().map(|(a, _)| *a).min(),
        records
            .iter()
            .map(|(a, d)| *a + u32::try_from(d.len()).unwrap_or(u32::MAX))
            .max(),
    ) else {
        return Err(FlashError::Hex("the file holds no data".into()));
    };
    let length = usize::try_from(end - start).map_err(|_| FlashError::Hex("too large".into()))?;
    if length > 1024 * 1024 {
        return Err(FlashError::Hex(format!(
            "the image would be {length} bytes, more than any Neuron holds"
        )));
    }
    let mut bytes = vec![0xFF; length];
    for (address, data) in records {
        let at = usize::try_from(address - start).unwrap_or(usize::MAX);
        bytes[at..at + data.len()].copy_from_slice(&data);
    }
    Ok(Image { start, bytes })
}

/// What the installer reports as it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// The keyboard was asked to restart into its bootloader.
    Restarting,
    /// The bootloader answered; erasing.
    Erasing,
    /// Writing: bytes done, of the total.
    Writing { done: usize, total: usize },
    /// The application was started; waiting for the keyboard.
    Starting,
    /// The keyboard is back, reporting this version.
    Done { version: String },
}

/// The time the bootloader gets to answer an erase.
const ERASE_TIMEOUT: Duration = Duration::from_secs(20);
/// The time it gets to answer anything else.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(2);

/// Sends one command and waits for the bootloader's answer: at least one
/// byte, then whatever follows within a moment, which is dropped.
fn command<T: Read + Write>(
    stream: &mut T,
    text: &str,
    timeout: Duration,
) -> Result<Vec<u8>, FlashError> {
    stream.write_all(text.as_bytes())?;
    stream.flush()?;
    let started = Instant::now();
    let mut answer = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => {}
            Ok(n) => {
                answer.extend_from_slice(&chunk[..n]);
                // One byte is the answer; give the rest of a burst a moment.
                let settle = Instant::now();
                while settle.elapsed() < Duration::from_millis(50) {
                    match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => answer.extend_from_slice(&chunk[..n]),
                        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => break,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e.into()),
                    }
                }
                return Ok(answer);
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
        if started.elapsed() >= timeout {
            let name = text.trim_end_matches('#');
            return Err(FlashError::NoAnswer(
                name.chars().take(1).collect::<String>(),
            ));
        }
    }
}

/// Writes `image` to a Neuron that is in its bootloader, over `stream`,
/// then starts it. The stream's reads should time out quickly (a serial
/// port opened with a timeout of a few hundred milliseconds).
pub fn flash_image<T: Read + Write>(
    stream: &mut T,
    image: &Image,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), FlashError> {
    image.check()?;
    progress(Progress::Erasing);
    command(stream, &format!("E{:08X}#", image.start), ERASE_TIMEOUT)?;
    let total = image.bytes.len();
    let mut done = 0usize;
    let mut address = image.start;
    progress(Progress::Writing { done, total });
    for packet in image.bytes.chunks(PACKET) {
        command(stream, &format!("U{:08X}#", packet.len()), ANSWER_TIMEOUT)?;
        for piece in packet.chunks(200) {
            stream.write_all(piece)?;
        }
        stream.flush()?;
        command(
            stream,
            &format!("W{:08X},{:08X}#", address, packet.len()),
            ANSWER_TIMEOUT,
        )?;
        done += packet.len();
        address += u32::try_from(packet.len()).unwrap_or(0);
        progress(Progress::Writing { done, total });
    }
    std::thread::sleep(Duration::from_millis(300));
    stream.write_all(b"S#")?;
    stream.flush()?;
    progress(Progress::Starting);
    Ok(())
}

impl<T: Read + Write> Focus<T> {
    /// Asks the running keyboard to restart its Neuron into the
    /// bootloader, the way Bazecor's firmware update does: `upgrade.start`
    /// first, which the firmware requires, then `upgrade.neuron`, which
    /// gets no answer because the Neuron restarts. These two commands are
    /// sent by this function only, never through the general write path.
    pub fn restart_into_bootloader(&mut self) -> Result<(), FocusError> {
        if !self.has("upgrade.neuron")? || !self.has("upgrade.start")? {
            return Err(FocusError::Refused("upgrade.neuron".into()));
        }
        self.exchange("upgrade.start")?;
        self.stream.write_all(b"upgrade.neuron\n")?;
        self.stream.flush()?;
        Ok(())
    }
}

/// The serial port of a Neuron in its bootloader, if one is connected.
pub fn find_bootloader_port() -> Option<String> {
    let mut ports: Vec<String> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter(|port| !port.port_name.contains("/tty."))
        .filter_map(|port| match port.port_type {
            serialport::SerialPortType::UsbPort(usb)
                if usb.vid == VENDOR && usb.pid == BOOTLOADER_PRODUCT =>
            {
                Some(port.port_name)
            }
            _ => None,
        })
        .collect();
    ports.sort();
    ports.into_iter().next()
}

fn wait_for<F: FnMut() -> Option<String>>(mut find: F, timeout: Duration) -> Option<String> {
    let started = Instant::now();
    loop {
        if let Some(found) = find() {
            return Some(found);
        }
        if started.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// Installs `hex` (the text of `Wireless_neuron.hex`) on the Neuron of the
/// connected keyboard of `board`: restarts it into the bootloader through
/// the keyboard's Focus port, flashes, starts it, and waits for the
/// keyboard to come back, returning the version it then reports.
///
/// If the Neuron is already in its bootloader (its light pulsing blue),
/// the restart is skipped.
pub fn install(
    board: &kc_boards::Board,
    device: Option<&kc_model::Device>,
    hex: &str,
    progress: &mut dyn FnMut(Progress),
) -> Result<String, FlashError> {
    let image = parse_hex(hex)?;
    image.check()?;
    const BOOTLOADER_WAIT: u64 = 20;
    const KEYBOARD_WAIT: u64 = 30;

    let bootloader = match find_bootloader_port() {
        Some(port) => port,
        None => {
            let port = crate::find_port(board, device).ok_or(FocusError::NotFound)?;
            let mut focus = crate::open(&port)?;
            progress(Progress::Restarting);
            focus.restart_into_bootloader()?;
            drop(focus);
            wait_for(find_bootloader_port, Duration::from_secs(BOOTLOADER_WAIT))
                .ok_or(FlashError::NoBootloader(BOOTLOADER_WAIT))?
        }
    };
    // The bootloader needs a moment after enumerating before it listens.
    std::thread::sleep(Duration::from_millis(500));
    let mut stream = serialport::new(&bootloader, 115_200)
        .timeout(Duration::from_millis(200))
        .open()
        .map_err(|e| FlashError::Io(std::io::Error::other(e.to_string())))?;
    flash_image(&mut stream, &image, progress)?;
    drop(stream);

    let port = wait_for(
        || crate::find_port(board, device),
        Duration::from_secs(KEYBOARD_WAIT),
    )
    .ok_or(FlashError::NoKeyboard(KEYBOARD_WAIT))?;
    // Give the firmware its first second to settle before asking it.
    std::thread::sleep(Duration::from_secs(1));
    let mut focus = crate::open(&port)?;
    let version = focus.read_required("version")?.trim().to_string();
    progress(Progress::Done {
        version: version.clone(),
    });
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// A bootloader that records what it is told and answers `A`.
    struct Bootloader {
        incoming: Vec<u8>,
        answers: VecDeque<u8>,
        pub commands: Vec<String>,
        pub flash: Vec<u8>,
        pub erased_from: Option<u32>,
        pending: Vec<u8>,
        expected: usize,
        started: bool,
    }

    impl Bootloader {
        fn new() -> Self {
            Self {
                incoming: Vec::new(),
                answers: VecDeque::new(),
                commands: Vec::new(),
                flash: vec![0xEE; 0x80000],
                erased_from: None,
                pending: Vec::new(),
                expected: 0,
                started: false,
            }
        }

        fn consume(&mut self) {
            loop {
                if self.expected > 0 {
                    let take = self.expected.min(self.incoming.len());
                    let bytes: Vec<u8> = self.incoming.drain(..take).collect();
                    self.pending.extend(bytes);
                    self.expected -= take;
                    if self.expected > 0 {
                        return;
                    }
                }
                let Some(end) = self.incoming.iter().position(|b| *b == b'#') else {
                    return;
                };
                let text = String::from_utf8(self.incoming.drain(..=end).collect()).unwrap();
                let text = text.trim_end_matches('#').to_string();
                self.commands.push(text.clone());
                let hex = |s: &str| u32::from_str_radix(s, 16).unwrap();
                match text.as_bytes()[0] {
                    b'E' => {
                        let from = hex(&text[1..]);
                        self.erased_from = Some(from);
                        for b in &mut self.flash[from as usize..APPLICATION_END as usize] {
                            *b = 0xFF;
                        }
                    }
                    b'U' => self.expected = hex(&text[1..]) as usize,
                    b'W' => {
                        let (addr, size) = text[1..].split_once(',').unwrap();
                        let (addr, size) = (hex(addr) as usize, hex(size) as usize);
                        assert_eq!(self.pending.len(), size, "W size matches U size");
                        self.flash[addr..addr + size].copy_from_slice(&self.pending);
                        self.pending.clear();
                    }
                    b'S' => {
                        self.started = true;
                        return;
                    }
                    other => panic!("unknown command {other}"),
                }
                self.answers.push_back(b'A');
            }
        }
    }

    impl Read for Bootloader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.answers.pop_front() {
                Some(b) => {
                    buf[0] = b;
                    Ok(1)
                }
                None => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "nothing")),
            }
        }
    }

    impl Write for Bootloader {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.incoming.extend_from_slice(buf);
            self.consume();
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn record(address: u16, kind: u8, data: &[u8]) -> String {
        let mut bytes = vec![data.len() as u8, (address >> 8) as u8, address as u8, kind];
        bytes.extend_from_slice(data);
        let sum = bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b));
        bytes.push(0u8.wrapping_sub(sum));
        format!(
            ":{}",
            bytes.iter().map(|b| format!("{b:02X}")).collect::<String>()
        )
    }

    fn hex_at(start: u32, data: &[u8]) -> String {
        let mut lines = vec![record(0, 0x04, &(start >> 16).to_be_bytes()[2..])];
        for (i, chunk) in data.chunks(16).enumerate() {
            let offset = (start & 0xFFFF) as u16 + (i * 16) as u16;
            lines.push(record(offset, 0x00, chunk));
        }
        lines.push(record(0, 0x01, &[]));
        lines.join("\n") + "\n"
    }

    #[test]
    fn parses_hex_with_extended_addresses_and_fills_gaps() {
        let text = format!(
            "{}{}",
            hex_at(0x27000, &[1, 2, 3, 4]).trim_end_matches(":00000001FF\n"),
            hex_at(0x27010, &[9, 9]).trim_start_matches(":020000040002F8\n")
        );
        let image = parse_hex(&text).unwrap();
        assert_eq!(image.start, 0x27000);
        assert_eq!(image.bytes.len(), 0x12);
        assert_eq!(&image.bytes[..4], &[1, 2, 3, 4]);
        assert!(image.bytes[4..16].iter().all(|b| *b == 0xFF));
        assert_eq!(&image.bytes[16..], &[9, 9]);
        assert!(image.check().is_ok());
    }

    #[test]
    fn rejects_broken_and_misplaced_images() {
        assert!(matches!(
            parse_hex(":10000000FFFFFFFF\n"),
            Err(FlashError::Hex(_))
        ));
        // A record whose checksum does not add up.
        let wrong = hex_at(0x27000, &[1]).replacen(":0170000001", ":0170000002", 1);
        assert!(matches!(parse_hex(&wrong), Err(FlashError::Hex(_))));
        let elsewhere = parse_hex(&hex_at(0x30000, &[1, 2])).unwrap();
        assert!(matches!(
            elsewhere.check(),
            Err(FlashError::WrongStart { .. })
        ));
        let huge = Image {
            start: 0x2E000,
            bytes: vec![0; (APPLICATION_END - 0x2E000 + 1) as usize],
        };
        assert!(matches!(huge.check(), Err(FlashError::TooLarge { .. })));
    }

    #[test]
    fn flashes_in_packets_like_bazecor() {
        let data: Vec<u8> = (0..(PACKET + 300)).map(|i| (i % 251) as u8).collect();
        let image = parse_hex(&hex_at(0x27000, &data)).unwrap();
        let mut bootloader = Bootloader::new();
        let mut seen = Vec::new();
        flash_image(&mut bootloader, &image, &mut |p| seen.push(p)).unwrap();
        assert_eq!(bootloader.erased_from, Some(0x27000));
        assert_eq!(
            bootloader.commands,
            vec![
                "E00027000".to_string(),
                format!("U{:08X}", PACKET),
                format!("W00027000,{:08X}", PACKET),
                "U0000012C".to_string(),
                "W00028000,0000012C".to_string(),
                "S".to_string(),
            ]
        );
        assert!(bootloader.started);
        assert_eq!(&bootloader.flash[0x27000..0x27000 + data.len()], &data[..]);
        assert_eq!(seen.first(), Some(&Progress::Erasing));
        assert_eq!(seen.last(), Some(&Progress::Starting));
        assert!(seen.contains(&Progress::Writing {
            done: data.len(),
            total: data.len()
        }));
    }

    #[test]
    fn a_silent_bootloader_is_an_error() {
        struct Silent;
        impl Read for Silent {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "nothing"))
            }
        }
        impl Write for Silent {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let image = Image {
            start: 0x2E000,
            bytes: vec![1, 2, 3],
        };
        let err = command(&mut Silent, "U00000003#", Duration::from_millis(10)).unwrap_err();
        assert!(matches!(err, FlashError::NoAnswer(ref c) if c == "U"));
        assert!(image.check().is_ok());
    }
}
