//! `CastMessage` (Chromium's `cast_channel.proto`) by hand: seven fields, so
//! a protobuf crate and its code generator aren't worth it.
//!
//! ```text
//! message CastMessage {
//!   required ProtocolVersion protocol_version = 1;  // CASTV2_1_0 = 0
//!   required string source_id = 2;
//!   required string destination_id = 3;
//!   required string namespace = 4;
//!   required PayloadType payload_type = 5;          // STRING = 0, BINARY = 1
//!   optional string payload_utf8 = 6;
//!   optional bytes payload_binary = 7;
//! }
//! ```
//!
//! On the wire each message is a 4-byte big-endian length and the encoded
//! message.

use anyhow::{Context, Result, bail, ensure};

/// Messages above this are refused (the protocol caps them at 64 KiB).
pub const MAX_MESSAGE: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CastMessage {
    pub source: String,
    pub destination: String,
    pub namespace: String,
    /// The JSON payload; binary payloads (device auth) arrive as None.
    pub payload: Option<String>,
}

impl CastMessage {
    pub fn new(source: &str, destination: &str, namespace: &str, payload: String) -> Self {
        CastMessage {
            source: source.to_owned(),
            destination: destination.to_owned(),
            namespace: namespace.to_owned(),
            payload: Some(payload),
        }
    }

    /// The length-prefixed frame.
    pub fn frame(&self) -> Result<Vec<u8>> {
        // Bound input before allocating the encoded copy. Field tags and
        // lengths add a little overhead, checked again after encoding.
        let content_len = [
            self.source.len(),
            self.destination.len(),
            self.namespace.len(),
            self.payload.as_ref().map_or(0, String::len),
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .context("Cast message length overflow")?;
        ensure!(
            content_len <= MAX_MESSAGE,
            "Cast message exceeds {MAX_MESSAGE} bytes"
        );
        let mut body = Vec::new();
        varint_field(&mut body, 1, 0);
        bytes_field(&mut body, 2, self.source.as_bytes());
        bytes_field(&mut body, 3, self.destination.as_bytes());
        bytes_field(&mut body, 4, self.namespace.as_bytes());
        varint_field(&mut body, 5, 0);
        if let Some(payload) = &self.payload {
            bytes_field(&mut body, 6, payload.as_bytes());
        }
        ensure!(
            body.len() <= MAX_MESSAGE,
            "Cast message exceeds {MAX_MESSAGE} bytes"
        );
        let size = u32::try_from(body.len()).context("Cast frame length")?;
        let mut frame = size.to_be_bytes().to_vec();
        frame.extend(body);
        Ok(frame)
    }

    /// Decodes a message body (without the length prefix). Unknown fields
    /// are skipped, as protobuf requires.
    pub fn decode(mut buf: &[u8]) -> Result<CastMessage> {
        let mut msg = CastMessage {
            source: String::new(),
            destination: String::new(),
            namespace: String::new(),
            payload: None,
        };
        while !buf.is_empty() {
            let key = varint(&mut buf)?;
            let (field, wire) = (key >> 3, key & 7);
            match wire {
                0 => {
                    varint(&mut buf)?;
                }
                2 => {
                    let len = usize::try_from(varint(&mut buf)?).context("Cast field length")?;
                    if len > buf.len() {
                        bail!("field {field} runs past the message");
                    }
                    let (value, rest) = buf.split_at(len);
                    buf = rest;
                    let text = || String::from_utf8_lossy(value).into_owned();
                    match field {
                        2 => msg.source = text(),
                        3 => msg.destination = text(),
                        4 => msg.namespace = text(),
                        6 => msg.payload = Some(text()),
                        _ => {}
                    }
                }
                5 if buf.len() >= 4 => buf = &buf[4..],
                1 if buf.len() >= 8 => buf = &buf[8..],
                _ => bail!("bad wire type {wire} for field {field}"),
            }
        }
        Ok(msg)
    }
}

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        // The wire format takes the low seven bits and sets the continuation bit.
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8); // The loop leaves at most seven bits.
}

fn varint_field(out: &mut Vec<u8>, field: u64, value: u64) {
    put_varint(out, field << 3);
    put_varint(out, value);
}

fn bytes_field(out: &mut Vec<u8>, field: u64, value: &[u8]) {
    put_varint(out, (field << 3) | 2);
    put_varint(out, value.len() as u64);
    out.extend_from_slice(value);
}

fn varint(buf: &mut &[u8]) -> Result<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let Some((&byte, rest)) = buf.split_first() else {
            bail!("truncated varint");
        };
        *buf = rest;
        if shift == 63 && byte > 1 {
            bail!("varint exceeds u64");
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    bail!("varint too long")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connect_message_has_the_known_bytes() {
        let msg = CastMessage::new(
            "sender-0",
            "receiver-0",
            "urn:x-cast:com.google.cast.tp.connection",
            r#"{"type":"CONNECT"}"#.into(),
        );
        let frame = msg.frame().unwrap();
        // Bytes as protoc encodes this message (fields in order, both
        // required enums present as zero).
        let mut want = vec![0x08, 0x00, 0x12, 0x08];
        want.extend(b"sender-0");
        want.extend([0x1a, 0x0a]);
        want.extend(b"receiver-0");
        want.extend([0x22, 0x28]);
        want.extend(b"urn:x-cast:com.google.cast.tp.connection");
        want.extend([0x28, 0x00, 0x32, 0x12]);
        want.extend(br#"{"type":"CONNECT"}"#);
        assert_eq!(&frame[4..], &want[..]);
        assert_eq!(
            u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize,
            want.len()
        );
        assert_eq!(CastMessage::decode(&frame[4..]).unwrap(), msg);
    }

    #[test]
    fn long_payloads_and_unknown_fields_decode() {
        let payload = "x".repeat(300);
        let msg = CastMessage::new("a", "b", "c", payload.clone());
        let mut body = msg.frame().unwrap()[4..].to_vec();
        // Field 7 (bytes) and a fixed32 field 9: skipped.
        body.extend([0x3a, 0x02, 0xff, 0xfe, 0x4d, 1, 2, 3, 4]);
        let decoded = CastMessage::decode(&body).unwrap();
        assert_eq!(decoded.payload.as_deref(), Some(payload.as_str()));
        assert!(CastMessage::decode(&[0x12, 0x05, b'a']).is_err());
    }

    #[test]
    fn rejects_oversized_frames_and_overflowing_varints() {
        let msg = CastMessage::new("a", "b", "c", "x".repeat(MAX_MESSAGE));
        assert!(msg.frame().is_err());
        let mut overflow = vec![0xff; 9];
        overflow.push(2);
        assert!(varint(&mut overflow.as_slice()).is_err());
        let mut largest = vec![0xff; 9];
        largest.push(1);
        assert_eq!(varint(&mut largest.as_slice()).unwrap(), u64::MAX);
        let mut length = vec![0x12];
        length.extend(largest);
        assert!(CastMessage::decode(&length).is_err());
    }
}
