use std::fmt;

/// Errors raised while reading a PML file.
#[derive(Debug)]
pub enum PmlError {
    Io(std::io::Error),
    /// The file does not start with the `PML_` signature.
    NotPml,
    /// The format version is one this reader does not understand.
    UnsupportedVersion(u32),
    /// A structure extends past the end of the file or of its enclosing record.
    Truncated,
}

impl fmt::Display for PmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PmlError::Io(e) => write!(f, "I/O error: {e}"),
            PmlError::NotPml => f.write_str("not a Process Monitor log (missing PML_ signature)"),
            PmlError::UnsupportedVersion(v) => {
                write!(f, "unsupported PML format version {v} (only version 9 is supported)")
            }
            PmlError::Truncated => f.write_str("PML file is truncated or corrupt"),
        }
    }
}

impl std::error::Error for PmlError {}

impl From<std::io::Error> for PmlError {
    fn from(e: std::io::Error) -> Self {
        PmlError::Io(e)
    }
}
