//! Core data types shared across the pipeline.

/// Logcat priority level. `S` (Silent) is included for completeness.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Level {
    Verbose,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
    Silent,
}

impl Level {
    pub fn from_char(c: char) -> Option<Self> {
        Some(match c {
            'V' => Self::Verbose,
            'D' => Self::Debug,
            'I' => Self::Info,
            'W' => Self::Warn,
            'E' => Self::Error,
            'F' => Self::Fatal,
            'S' => Self::Silent,
            _ => return None,
        })
    }

    pub fn letter(self) -> char {
        match self {
            Self::Verbose => 'V',
            Self::Debug => 'D',
            Self::Info => 'I',
            Self::Warn => 'W',
            Self::Error => 'E',
            Self::Fatal => 'F',
            Self::Silent => 'S',
        }
    }
}

/// One parsed `-v threadtime` logcat record. The timestamp is kept as the raw
/// string (we never need to do arithmetic on it, only display it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRecord {
    pub ts: String,
    pub pid: u32,
    pub tid: u32,
    /// Present only when the device emits the `uid` column (modern logd / `,uid`).
    pub uid: Option<String>,
    pub level: Level,
    pub tag: String,
    pub msg: String,
}
