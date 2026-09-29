use std::fmt;
use std::io;

/// Errors surfaced by the core library. Kept small and dependency-free so the
/// whole toolchain can be built offline.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Config(String),
    Parse(String),
    NotFound(String),
    Usage(String),
    External(i32),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Config(m) => write!(f, "{m}"),
            Error::Parse(m) => write!(f, "{m}"),
            Error::NotFound(m) => write!(f, "{m}"),
            Error::Usage(m) => write!(f, "{m}"),
            Error::External(code) => write!(f, "external command exited with status {code}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Error::Io(value)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Convenience constructors, mirroring `todo.sh`'s `die`/`dieWithHelp`.
impl Error {
    pub fn usage(msg: impl Into<String>) -> Self {
        Error::Usage(msg.into())
    }
    pub fn config(msg: impl Into<String>) -> Self {
        Error::Config(msg.into())
    }
}
