//! Ini files read by the original's own reader (`Rus_*.ini`, `_Global.ini`, `_Sounds.ini`),
//! with its rules (saves-data.md §1), not the Windows ini API's:
//!
//! - A carriage return ends a line and the byte after it is skipped unseen (taken for the line
//!   feed); every other byte below 0x0D is dropped wherever it is, tabs and line feeds
//!   included, so a file with bare line feeds reads as one line. Trailing spaces are cut.
//!   Nothing is a comment: `//` lines are kept like any other (their keys never match).
//! - A line is a section header when it holds a `[` and, after it, a `]`, anywhere in the
//!   line; the name is the text between them. Section names may repeat (every spell is
//!   `[<same name>]`), so sections are kept as a list in file order. Lines before the first
//!   header belong to no section.
//! - Every line with an `=` is split at the first `=`: the key is the text before it, exactly
//!   (case-sensitive, not trimmed: `Cost =5` is the key `Cost `), the value everything after
//!   it, untrimmed. When a key or a section name repeats, the **last** one wins.
//! - Integers are read loosely ([`loose_int`]); a missing key reads as 0.
//!
//! Razdor's own `data/*.ini` go through the same rules; [`Ini::parse`] also ends a line at a
//! bare line feed, since those files and the tests are written with them.

use super::text;

/// One `[name]` section.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    /// `(key, value)` pairs in file order, as written (untrimmed).
    pub entries: Vec<(String, String)>,
    /// Lines that are neither headers nor `key=value`.
    pub lines: Vec<String>,
}

impl Section {
    /// The value of `key`: an exact, case-sensitive match, the last one when it repeats
    /// (0x472440).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// The value of `key`, or `None` when it is absent or empty.
    pub fn get_nonempty(&self, key: &str) -> Option<&str> {
        self.get(key).filter(|v| !v.is_empty())
    }

    /// The value of `key` read as the original reads an integer; `None` when absent or
    /// empty (the original reads 0 then).
    pub fn get_int(&self, key: &str) -> Option<i32> {
        self.get_nonempty(key).map(loose_int)
    }

    /// The integer of `key`; absent or empty is 0, as the original reads it (0x47274c).
    pub fn int(&self, key: &str) -> i32 {
        self.get_int(key).unwrap_or(0)
    }

    /// Field `n` (1-based) of a comma-separated value (0x472548): field 1 is the text up to
    /// the first comma, field n the text after the (n−1)-th comma up to the next one; past
    /// the end, or a missing key, is empty.
    pub fn field(&self, key: &str, n: usize) -> &str {
        self.get(key).and_then(|v| v.split(',').nth(n.max(1) - 1)).unwrap_or("")
    }

    /// Field `n` as an integer (0x472874); an empty field reads as 0.
    pub fn field_int(&self, key: &str, n: usize) -> i32 {
        match self.field(key, n) {
            "" => 0,
            f => loose_int(f),
        }
    }
}

/// The original's integer reader (0x471de8): every space is removed, then every decimal
/// digit is taken in order and **every other character is skipped** (`12a3` reads 123, `1.5`
/// reads 15, `3 // note 7` reads 37); a `-` anywhere negates the result. Only the first
/// (length mod 256) characters are scanned (its loop counter is a byte), and the value wraps
/// at 32 bits. The empty string reads 0.
pub fn loose_int(s: &str) -> i32 {
    let packed: Vec<char> = s.chars().filter(|&c| c != ' ').collect();
    let scanned = packed.len() % 256;
    let v = packed[..scanned]
        .iter()
        .filter_map(|c| c.to_digit(10))
        .fold(0i32, |v, d| v.wrapping_mul(10).wrapping_add(d as i32));
    if s.contains('-') {
        v.wrapping_neg()
    } else {
        v
    }
}

/// A parsed ini file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ini {
    pub sections: Vec<Section>,
}

impl Ini {
    /// Parse already decoded text. A line ends at a line feed (with or without a carriage
    /// return before it), then the original's rules apply.
    pub fn parse(text: &str) -> Ini {
        Ini::from_lines(text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).chars().filter(|&c| c >= '\r').collect()))
    }

    /// Parse raw cp1251 bytes, splitting lines as the original's reader does (0x473a6c): a
    /// carriage return ends the line and the next byte is skipped unseen; any other byte
    /// below 0x0D is dropped.
    pub fn from_cp1251(bytes: &[u8]) -> Ini {
        let mut lines = Vec::new();
        let mut cur = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\r' => {
                    lines.push(text::decode(&std::mem::take(&mut cur)));
                    i += 1;
                }
                b if b > b'\r' => cur.push(b),
                _ => {}
            }
            i += 1;
        }
        if !cur.is_empty() {
            lines.push(text::decode(&cur));
        }
        Ini::from_lines(lines.into_iter())
    }

    fn from_lines(lines: impl Iterator<Item = String>) -> Ini {
        let mut sections: Vec<Section> = Vec::new();
        for line in lines {
            let line = line.trim_end_matches(' ');
            if let Some(name) = header(line) {
                sections.push(Section { name: name.to_string(), ..Section::default() });
                continue;
            }
            // Lines before the first header are in no section.
            let Some(cur) = sections.last_mut() else { continue };
            match line.split_once('=') {
                Some((k, v)) => cur.entries.push((k.to_string(), v.to_string())),
                None if !line.is_empty() => cur.lines.push(line.to_string()),
                None => {}
            }
        }
        Ini { sections }
    }

    /// The last section called exactly `name` (0x472390).
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().rev().find(|s| s.name == name)
    }

    /// The section the original's reader has selected after selecting `name` when `current`
    /// was selected: the last exact match, else **`current` stays selected** (0x472390), so
    /// the reads that follow come from the wrong section.
    pub fn select(&self, name: &str, current: Option<usize>) -> Option<usize> {
        self.sections.iter().rposition(|s| s.name == name).or(current)
    }
}

/// The name of a section header line: the text between the first `[` and the first `]`,
/// when the `]` comes after the `[`, anywhere in the line.
fn header(line: &str) -> Option<&str> {
    let open = line.find('[')?;
    let close = line.find(']')?;
    (open < close).then(|| &line[open + 1..close])
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "// header comment\r\n\
        [A]\r\n\
        Key=1\r\n\
        Empty=\r\n\
        \r\n\
        // comment = not a key\r\n\
        List= 1, 2 ,-3\r\n\
        Text = a=b\r\n\
        ----------\r\n\
        [B]\r\n\
        Key=first\r\n\
        key=second\r\n\
        Key=third\r\n\
        [A]\r\n\
        Key=2\r\n";

    #[test]
    fn keeps_duplicate_sections_in_order_and_selects_the_last() {
        let ini = Ini::parse(SAMPLE);
        let names: Vec<&str> = ini.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["A", "B", "A"]);
        assert_eq!(ini.section("A").unwrap().get("Key"), Some("2"), "the last [A]");
        assert_eq!(ini.section("a"), None, "section names are case-sensitive");
        assert_eq!(ini.select("A", None), Some(2));
        // A missing section leaves the previous one selected.
        assert_eq!(ini.select("Missing", Some(1)), Some(1));
        assert_eq!(ini.select("Missing", None), None);
    }

    #[test]
    fn keys_are_exact_untrimmed_and_the_last_wins() {
        let ini = Ini::parse(SAMPLE);
        let a = &ini.sections[0];
        assert_eq!(a.get("Empty"), Some(""));
        assert_eq!(a.get_nonempty("Empty"), None);
        assert_eq!(a.get("Missing"), None);
        assert_eq!(a.get("Text"), None, "the key is `Text `");
        assert_eq!(a.get("Text "), Some(" a=b"), "the value is untrimmed, split at the first =");
        assert_eq!(a.get("// comment "), Some(" not a key"), "comments are lines like any other");
        assert_eq!(a.lines, ["----------"]);
        let b = &ini.sections[1];
        assert_eq!(b.get("Key"), Some("third"));
        assert_eq!(b.get("KEY"), None);
        assert_eq!(b.get("key"), Some("second"));
    }

    #[test]
    fn integers_are_read_loosely() {
        // saves-data.md §1 rule 5.
        assert_eq!(loose_int("12a3"), 123);
        assert_eq!(loose_int("1.5"), 15);
        assert_eq!(loose_int("3 // note 7"), 37);
        assert_eq!(loose_int(" 4 2 "), 42);
        assert_eq!(loose_int("5-"), -5, "a minus anywhere negates");
        assert_eq!(loose_int("-"), 0);
        assert_eq!(loose_int("abc"), 0);
        assert_eq!(loose_int(""), 0);
        assert_eq!(loose_int("4294967297"), 1, "wraps at 32 bits");
        // Only the first (length mod 256) characters count: 256 digits read none.
        assert_eq!(loose_int(&"1".repeat(256)), 0);
        assert_eq!(loose_int(&format!("{}7", "x".repeat(256))), 0, "257 characters: only the first is scanned");
        let ini = Ini::parse("[S]\nA=12a3\nB=\nC= -7\n");
        let s = &ini.sections[0];
        assert_eq!((s.get_int("A"), s.get_int("B"), s.get_int("C"), s.get_int("D")), (Some(123), None, Some(-7), None));
        assert_eq!((s.int("B"), s.int("D")), (0, 0), "empty or missing reads 0");
    }

    #[test]
    fn list_fields() {
        let ini = Ini::parse("[S]\nList= 1, 2 ,-3\nShort=4,5\n");
        let s = &ini.sections[0];
        assert_eq!((s.field("List", 1), s.field("List", 2), s.field("List", 3)), (" 1", " 2 ", "-3"));
        assert_eq!([1, 2, 3, 4].map(|n| s.field_int("List", n)), [1, 2, -3, 0]);
        assert_eq!([1, 2, 3].map(|n| s.field_int("Short", n)), [4, 5, 0], "a field past the end reads 0");
        assert_eq!(s.field_int("Missing", 1), 0);
    }

    #[test]
    fn headers_anywhere_in_a_line() {
        let ini = Ini::parse("X=1\nnote [Real] here\nK=v\n]not[ a header\nL=w\n");
        assert_eq!(ini.sections.len(), 1, "lines before the first header are in no section");
        let s = &ini.sections[0];
        assert_eq!(s.name, "Real");
        assert_eq!((s.get("K"), s.get("L")), (Some("v"), Some("w")));
        assert_eq!(s.lines, ["]not[ a header"]);
    }

    #[test]
    fn original_line_rules() {
        // A CR ends the line and the byte after it is skipped, whatever it is; tabs and other
        // control bytes vanish; trailing spaces are cut.
        let ini = Ini::from_cp1251(b"[S]\r\nA=1\t2  \rXB=3\rYC=4\r\n");
        let s = &ini.sections[0];
        assert_eq!((s.get("A"), s.get("B"), s.get("C")), (Some("12"), Some("3"), Some("4")));
        // A file with bare line feeds reads as a single line.
        let lf = Ini::from_cp1251(b"[S]\nA=1\nB=2\n");
        assert_eq!(lf.sections.len(), 1);
        assert_eq!(lf.sections[0].name, "S");
        assert!(lf.sections[0].entries.is_empty(), "the keys sit on the header's line");
    }

    #[test]
    fn decodes_cp1251() {
        // "[Имя]\r\nK=Да" in cp1251.
        let bytes = [b'[', 0xC8, 0xEC, 0xFF, b']', b'\r', b'\n', b'K', b'=', 0xC4, 0xE0];
        let ini = Ini::from_cp1251(&bytes);
        assert_eq!(ini.sections[0].name, "Имя");
        assert_eq!(ini.sections[0].get("K"), Some("Да"));
    }

    #[test]
    fn empty_input() {
        assert!(Ini::parse("").sections.is_empty());
        assert!(Ini::parse("// only\r\n\r\n").sections.is_empty());
    }
}
