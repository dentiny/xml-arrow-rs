use std::{error::Error, io};

/// A result using this crate's [`ReadError`] type.
pub type Result<T> = std::result::Result<T, ReadError>;

/// The resource limit reached while reading XML.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    /// Total element nesting depth, including unselected elements.
    Depth,
    /// Serialized record bytes, including inherited namespace declarations.
    RecordBytes,
    /// Source bytes consumed for one XML event, including unselected content.
    EventBytes,
}

/// Configuration, XML input, resource-limit, or record conversion failure.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// Invalid options or record path; detected before input is read.
    #[error("invalid configuration: {0}")]
    Configuration(String),
    /// XML parsing, UTF-8, or input I/O error with the original source.
    #[error("XML input at byte {byte_offset}: {source}")]
    Xml {
        /// Source byte offset reached by the parser.
        byte_offset: u64,
        /// Original error; parser positions within a validated token are relative
        /// to that token, while `byte_offset` locates it in the input.
        source: Box<dyn Error + Send + Sync>,
    },
    /// An event, record, or document depth exceeded the configured limit.
    #[error("{kind:?} limit {limit} exceeded at byte {byte_offset}")]
    LimitExceeded {
        /// Limit that was exceeded.
        kind: Limit,
        /// Configured maximum.
        limit: usize,
        /// Source byte offset reached by the parser.
        byte_offset: u64,
    },
    /// A complete record failed Arrow conversion.
    #[error("record {record_index} at byte {byte_offset}: {source}")]
    Conversion {
        /// One-based index among selected records.
        record_index: u64,
        /// Byte offset of the selected record's opening tag.
        byte_offset: u64,
        /// Original conversion error, including field name and path if available.
        source: crate::Error,
    },
}

impl ReadError {
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
