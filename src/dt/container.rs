//! The `AIpf` container around `.DTm` maps: a 12-byte header and a bzip2 stream.
//!
//! | off | type | value |
//! |---|---|---|
//! | 0 | char[6] | `AIpf\r\n` |
//! | 6 | u16 | container version (19 in all shipped maps) |
//! | 8 | u32 | size of the uncompressed payload |
//! | 12 | … | bzip2 stream to EOF |

use super::DtError;
use std::io::{Read, Write};

/// Magic bytes at the start of a container.
pub const MAGIC: &[u8; 6] = b"AIpf\r\n";
/// Header length before the bzip2 stream.
pub const HEADER_LEN: usize = 12;

/// A decoded container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Container {
    pub version: u16,
    pub payload: Vec<u8>,
}

/// Parse a container and decompress its payload.
pub fn decode(bytes: &[u8]) -> Result<Container, DtError> {
    if bytes.len() < HEADER_LEN {
        if !MAGIC.starts_with(&bytes[..bytes.len().min(MAGIC.len())]) {
            return Err(DtError::BadMagic { what: "AIpf container" });
        }
        return Err(DtError::Truncated { what: "container header", offset: bytes.len() });
    }
    if &bytes[..6] != MAGIC {
        return Err(DtError::BadMagic { what: "AIpf container" });
    }
    let version = u16::from_le_bytes([bytes[6], bytes[7]]);
    let size = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    // Read at most one byte more than announced, so a lying header cannot make us inflate
    // an arbitrarily large stream, yet an oversized payload is still detected.
    let mut payload = Vec::with_capacity(size as usize);
    bzip2::read::BzDecoder::new(&bytes[HEADER_LEN..])
        .take(size as u64 + 1)
        .read_to_end(&mut payload)
        .map_err(|e| DtError::Bzip2(e.to_string()))?;
    if payload.len() != size as usize {
        return Err(DtError::PayloadSize { expected: size, actual: payload.len() });
    }
    Ok(Container { version, payload })
}

/// Build a container around `payload` (used by tests and future writers).
pub fn encode(version: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len() / 4);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    let mut enc = bzip2::write::BzEncoder::new(out, bzip2::Compression::best());
    enc.write_all(payload).expect("writing to a Vec cannot fail");
    enc.finish().expect("writing to a Vec cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let payload = b"MapLDV V.4\r\nhello hello hello".to_vec();
        let bytes = encode(19, &payload);
        assert_eq!(&bytes[..6], MAGIC);
        assert_eq!(&bytes[12..15], b"BZh");
        let c = decode(&bytes).unwrap();
        assert_eq!(c, Container { version: 19, payload });
    }

    #[test]
    fn empty_payload() {
        let c = decode(&encode(1, &[])).unwrap();
        assert!(c.payload.is_empty());
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = encode(19, b"x");
        bytes[0] = b'B';
        assert!(matches!(decode(&bytes), Err(DtError::BadMagic { .. })));
    }

    #[test]
    fn rejects_short_header() {
        assert!(matches!(decode(b"AIpf\r\n\x13"), Err(DtError::Truncated { .. })));
    }

    #[test]
    fn rejects_size_mismatch() {
        let mut bytes = encode(19, b"abcdef");
        bytes[8] = 5;
        assert!(matches!(decode(&bytes), Err(DtError::PayloadSize { expected: 5, actual: 6 })));
        bytes[8] = 9;
        assert!(matches!(decode(&bytes), Err(DtError::PayloadSize { expected: 9, actual: 6 })));
    }

    #[test]
    fn rejects_corrupt_stream() {
        let mut bytes = encode(19, b"some payload bytes");
        let n = bytes.len();
        bytes.truncate(n - 8);
        assert!(matches!(decode(&bytes), Err(DtError::Bzip2(_))));
    }
}
