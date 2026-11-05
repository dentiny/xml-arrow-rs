use std::{error::Error, io};

/// Result with [`ReadError`].
pub type Result<T> = std::result::Result<T, ReadError>;

/// XML input limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    /// Serialized XML bytes retained for inference.
    SampleBytes,
    /// Document element depth.
    Depth,
    /// Record bytes, including inherited namespaces.
    RecordBytes,
    /// Input bytes per event, including skipped content.
    EventBytes,
}

/// XML reader error.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// Invalid options or record path.
    #[error("invalid configuration: {0}")]
    Configuration(String),
    /// XML, UTF-8, or input error.
    #[error("XML input at byte {byte_offset}: {source}")]
    Xml {
        /// Input byte offset.
        byte_offset: u64,
        /// Underlying error; XML locations may be event-relative.
        source: Box<dyn Error + Send + Sync>,
    },
    /// Input limit exceeded.
    #[error("{kind:?} limit {limit} exceeded at byte {byte_offset}")]
    LimitExceeded {
        /// Exceeded limit.
        kind: Limit,
        /// Configured maximum.
        limit: usize,
        /// Input byte offset.
        byte_offset: u64,
    },
    /// Record conversion failed.
    #[error("record {record_index} at byte {byte_offset}: {source}")]
    Conversion {
        /// One-based index among selected records.
        record_index: u64,
        /// Record start byte offset.
        byte_offset: u64,
        /// Underlying conversion error.
        source: crate::Error,
    },
}

impl ReadError {
    pub(super) fn configuration(error: crate::Error) -> Self {
        Self::Configuration(match error {
            crate::Error::Configuration(message) => message,
            error => error.to_string(),
        })
    }

    pub(super) fn xml(byte_offset: u64, source: impl Error + Send + Sync + 'static) -> Self {
        Self::Xml {
            byte_offset,
            source: Box::new(source),
        }
    }

    pub(super) fn invalid(byte_offset: u64, message: &str) -> Self {
        Self::xml(
            byte_offset,
            io::Error::new(io::ErrorKind::InvalidData, message.to_owned()),
        )
    }

    pub(super) fn limit(kind: Limit, limit: usize, byte_offset: u64) -> Self {
        Self::LimitExceeded {
            kind,
            limit,
            byte_offset,
        }
    }
}
