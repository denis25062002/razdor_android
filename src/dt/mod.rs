//! Readers for the files of an installed *Discord Times* (Community Update).
//!
//! Pure data decoding: no macroquad and no game rules. Nothing here ships original content;
//! everything is read at runtime from the player's own install (see [`install`]).
//! Formats are documented in `docs/reference/dtm-format.md` and `docs/reference/mechanics.md`.
pub mod container;
pub mod data;
pub mod dtm;
pub mod ini;
pub mod install;
pub mod text;

use std::fmt;
use std::path::PathBuf;

/// Everything that can go wrong while reading the original's files.
#[derive(Debug)]
pub enum DtError {
    /// A file could not be read.
    Io { path: PathBuf, source: std::io::Error },
    /// `RAZDOR_DT_DIR` is not set.
    NoInstallDir,
    /// The file does not start with the expected magic bytes.
    BadMagic { what: &'static str },
    /// The data ends before a field or section that must be there.
    Truncated { what: &'static str, offset: usize },
    /// The bzip2 stream is corrupt.
    Bzip2(String),
    /// The decompressed payload does not have the size the container header announces.
    PayloadSize { expected: u32, actual: usize },
    /// A section's byte size is not a multiple of its record size.
    SectionSize { section: &'static str, size: u32, record: usize },
    /// The terrain RLE stream is malformed or does not expand to `W*H` cells.
    Terrain(String),
    /// The `\x08>-Text-` marker is not where the section sizes put it.
    TextMarker { offset: usize },
    /// The header's text offset disagrees with the section sizes.
    TextOffset { header: u32, computed: usize },
    /// A NUL-terminated string runs to the end of the data.
    UnterminatedString { offset: usize },
    /// Bytes remain after the last known part of the payload.
    TrailingBytes { offset: usize, count: usize },
    /// An ini value could not be interpreted.
    BadValue { section: String, key: String, value: String },
    /// A required ini key or section is missing.
    Missing { section: String, key: String },
}

impl fmt::Display for DtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DtError::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            DtError::NoInstallDir => write!(f, "RAZDOR_DT_DIR is not set"),
            DtError::BadMagic { what } => write!(f, "not a {what} (bad magic bytes)"),
            DtError::Truncated { what, offset } => write!(f, "{what} truncated at offset {offset:#x}"),
            DtError::Bzip2(e) => write!(f, "bzip2: {e}"),
            DtError::PayloadSize { expected, actual } => {
                write!(f, "payload is {actual} bytes, header says {expected}")
            }
            DtError::SectionSize { section, size, record } => {
                write!(f, "{section} section size {size} is not a multiple of {record}")
            }
            DtError::Terrain(e) => write!(f, "terrain: {e}"),
            DtError::TextMarker { offset } => write!(f, "text marker not found at {offset:#x}"),
            DtError::TextOffset { header, computed } => {
                write!(f, "text offset {header:#x} in header, {computed:#x} computed")
            }
            DtError::UnterminatedString { offset } => write!(f, "unterminated string at {offset:#x}"),
            DtError::TrailingBytes { offset, count } => write!(f, "{count} trailing bytes at {offset:#x}"),
            DtError::BadValue { section, key, value } => write!(f, "[{section}] {key}={value}: bad value"),
            DtError::Missing { section, key } => write!(f, "[{section}] {key} is missing"),
        }
    }
}

impl std::error::Error for DtError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DtError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
