//! Ini files as the original writes them (`Rus_*.ini`, `_Global.ini`).
//!
//! - `[Section]` headers; section names may repeat (every spell is `[<same name>]`), so
//!   sections are kept as a list in file order.
//! - `Key=Value` entries; values may be empty (`TimeWork=` means "no duration").
//! - Lines starting with `//` (or `;`) are comments. Other lines without `=` (separators such
//!   as `-----`) are kept as bare lines of their section.
//! - Keys are matched ignoring ASCII case, as the Windows ini API does (`_Global.ini` writes
//!   `DecSpellelemental`). When a key repeats inside a section, the first one wins, again as
//!   the Windows API does; all entries are still kept in order.
//! - Lines before the first header go to a section with an empty name.

use super::{text, DtError};

/// One `[name]` section.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    /// `(key, value)` pairs in file order, trimmed.
    pub entries: Vec<(String, String)>,
    /// Non-comment lines that are neither headers nor `key=value`.
    pub lines: Vec<String>,
}

impl Section {
    /// The first value of `key` (ASCII case-insensitive).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }

    /// The value of `key`, or `None` when it is absent or empty.
    pub fn get_nonempty(&self, key: &str) -> Option<&str> {
        self.get(key).filter(|v| !v.is_empty())
    }

    /// The value of `key` as an integer; `None` when absent or empty.
    pub fn get_int(&self, key: &str) -> Result<Option<i32>, DtError> {
        match self.get_nonempty(key) {
            None => Ok(None),
            Some(v) => v.parse().map(Some).map_err(|_| self.bad_value(key, v)),
        }
    }

    /// Like [`Section::get_int`], but absent or empty gives `default`.
    pub fn int_or(&self, key: &str, default: i32) -> Result<i32, DtError> {
        Ok(self.get_int(key)?.unwrap_or(default))
    }

    /// A comma-separated list of integers (`1,1,100,50,50`); `None` when absent or empty.
    pub fn get_int_list(&self, key: &str) -> Result<Option<Vec<i32>>, DtError> {
        match self.get_nonempty(key) {
            None => Ok(None),
            Some(v) => v
                .split(',')
                .map(|x| x.trim().parse().map_err(|_| self.bad_value(key, v)))
                .collect::<Result<Vec<i32>, _>>()
                .map(Some),
        }
    }

    /// An error for a value that could not be interpreted.
    pub fn bad_value(&self, key: &str, value: &str) -> DtError {
        DtError::BadValue { section: self.name.clone(), key: key.to_string(), value: value.to_string() }
    }
}

/// A parsed ini file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ini {
    pub sections: Vec<Section>,
}

impl Ini {
    /// Parse already decoded text.
    pub fn parse(text: &str) -> Ini {
        let mut sections: Vec<Section> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") || line.starts_with(';') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                sections.push(Section { name: name.to_string(), ..Section::default() });
                continue;
            }
            if sections.is_empty() {
                sections.push(Section::default());
            }
            let cur = sections.last_mut().expect("just ensured");
            match line.split_once('=') {
                Some((k, v)) => cur.entries.push((k.trim().to_string(), v.trim().to_string())),
                None => cur.lines.push(line.to_string()),
            }
        }
        Ini { sections }
    }

    /// Parse raw cp1251 bytes.
    pub fn from_cp1251(bytes: &[u8]) -> Ini {
        Ini::parse(&text::decode(bytes))
    }

    /// The first section called `name` (ASCII case-insensitive).
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }
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
        ; also a comment\r\n\
        Text = a=b\r\n\
        ----------\r\n\
        [B]\r\n\
        Key=first\r\n\
        key=second\r\n\
        [A]\r\n\
        Key=2\r\n";

    #[test]
    fn keeps_duplicate_sections_in_order() {
        let ini = Ini::parse(SAMPLE);
        let names: Vec<&str> = ini.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["A", "B", "A"]);
        assert_eq!(ini.sections[2].get("Key"), Some("2"));
        assert_eq!(ini.section("a").unwrap().get("Key"), Some("1"));
    }

    #[test]
    fn values_comments_and_bare_lines() {
        let ini = Ini::parse(SAMPLE);
        let a = &ini.sections[0];
        assert_eq!(a.get("Empty"), Some(""));
        assert_eq!(a.get_nonempty("Empty"), None);
        assert_eq!(a.get("Missing"), None);
        assert_eq!(a.get("Text"), Some("a=b"));
        assert_eq!(a.get("// comment"), None);
        assert_eq!(a.entries.len(), 4);
        assert_eq!(a.lines, ["----------"]);
        assert_eq!(a.get_int("Key").unwrap(), Some(1));
        assert_eq!(a.get_int("Empty").unwrap(), None);
        assert_eq!(a.int_or("Empty", 7).unwrap(), 7);
        assert_eq!(a.get_int_list("List").unwrap(), Some(vec![1, 2, -3]));
        assert!(matches!(a.get_int("Text"), Err(DtError::BadValue { .. })));
    }

    #[test]
    fn keys_ignore_case_and_first_wins() {
        let ini = Ini::parse(SAMPLE);
        let b = &ini.sections[1];
        assert_eq!(b.get("KEY"), Some("first"));
        assert_eq!(b.entries.len(), 2);
    }

    #[test]
    fn lines_before_any_header() {
        let ini = Ini::parse("X=1\n[S]\nY=2");
        assert_eq!(ini.sections.len(), 2);
        assert_eq!(ini.sections[0].name, "");
        assert_eq!(ini.sections[0].get("X"), Some("1"));
        assert_eq!(ini.section("S").unwrap().get_int("y").unwrap(), Some(2));
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
