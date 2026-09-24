//! Windows-1251 (cp1251) text, the encoding of every original data file and map string.

use encoding_rs::WINDOWS_1251;

/// Decode cp1251 bytes. Every byte value maps to a character, so this cannot fail.
pub fn decode(bytes: &[u8]) -> String {
    WINDOWS_1251.decode_without_bom_handling(bytes).0.into_owned()
}

/// Encode to cp1251. Characters with no cp1251 form become `?`.
pub fn encode(text: &str) -> Vec<u8> {
    WINDOWS_1251.encode(text).0.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_cyrillic() {
        // "Рус" in cp1251.
        assert_eq!(decode(&[0xD0, 0xF3, 0xF1]), "Рус");
        assert_eq!(decode(b"abc"), "abc");
    }

    #[test]
    fn roundtrips_every_byte() {
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(encode(&decode(&all)), all);
    }
}
