//! How Studio messages are delimited on a byte stream: a start byte, the
//! message with any special byte escaped, and an end byte.

const START: u8 = 0xAB;
const ESCAPE: u8 = 0xAC;
const END: u8 = 0xAD;

/// Wraps a message for sending.
pub fn encode(message: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(message.len() + 2);
    out.push(START);
    for &byte in message {
        if matches!(byte, START | ESCAPE | END) {
            out.push(ESCAPE);
        }
        out.push(byte);
    }
    out.push(END);
    out
}

/// Reassembles messages from bytes as they arrive.
#[derive(Debug, Default)]
pub struct Decoder {
    message: Vec<u8>,
    in_message: bool,
    escaped: bool,
}

impl Decoder {
    /// Feeds received bytes in and returns every message they complete.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut complete = Vec::new();
        for &byte in bytes {
            if self.escaped {
                self.message.push(byte);
                self.escaped = false;
                continue;
            }
            match byte {
                START => {
                    self.message.clear();
                    self.in_message = true;
                }
                ESCAPE if self.in_message => self.escaped = true,
                END if self.in_message => {
                    complete.push(std::mem::take(&mut self.message));
                    self.in_message = false;
                }
                // Bytes outside a frame are noise, such as log output.
                _ if self.in_message => self.message.push(byte),
                _ => {}
            }
        }
        complete
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_bytes_are_escaped_and_restored() {
        let message = [0x01, START, 0x02, ESCAPE, END, 0x03];
        let framed = encode(&message);
        assert_eq!(framed.len(), message.len() + 2 + 3);
        assert_eq!((framed[0], *framed.last().unwrap()), (START, END));
        assert_eq!(Decoder::default().feed(&framed), [message.to_vec()]);
    }

    #[test]
    fn messages_survive_being_split_and_surrounded_by_noise() {
        let (a, b) = (encode(&[1, 2, 3]), encode(&[END, 4]));
        let mut stream = vec![0x55, 0x66];
        stream.extend(&a);
        stream.extend([0x77]);
        stream.extend(&b);
        let mut decoder = Decoder::default();
        let mut messages = Vec::new();
        for chunk in stream.chunks(2) {
            messages.extend(decoder.feed(chunk));
        }
        assert_eq!(messages, [vec![1, 2, 3], vec![END, 4]]);
        assert!(
            encode(&[]) == [START, END]
                && Decoder::default().feed(&[START, END]) == [Vec::<u8>::new()]
        );
    }
}
