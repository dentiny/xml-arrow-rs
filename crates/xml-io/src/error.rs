use std::{error::Error, fmt};

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
#[derive(Debug)]
pub enum ReadError {
    /// Invalid options or record path; detected before input is read.
    Configuration(String),
    /// XML parsing, UTF-8, or input I/O error with the original source.
    Xml {
        /// Source byte offset reached by the parser.
        byte_offset: u64,
        /// Original error; parser positions within a validated token are relative
        /// to that token, while `byte_offset` locates it in the input.
        source: Box<dyn Error + Send + Sync>,
    },
    /// An event, record, or document depth exceeded the configured limit.
    LimitExceeded {
        /// Limit that was exceeded.
        kind: Limit,
        /// Configured maximum.
        limit: usize,
        /// Source byte offset reached by the parser.
        byte_offset: u64,
    },
    /// A complete record failed Arrow conversion.
    Conversion {
        /// One-based index among selected records.
        record_index: u64,
        /// Byte offset of the selected record's opening tag.
        byte_offset: u64,
        /// Original conversion error, including field name and path if available.
        source: xml_arrow::Error,
    },
}

impl ReadError {
    pub(crate) fn xml(byte_offset: u64, source: impl Error + Send + Sync + 'static) -> Self {
        Self::Xml {
            byte_offset,
            source: Box::new(source),
        }
    }
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => write!(f, "invalid configuration: {message}"),
            Self::Xml {
                byte_offset,
                source,
            } => write!(f, "XML input at byte {byte_offset}: {source}"),
            Self::LimitExceeded {
                kind,
                limit,
                byte_offset,
            } => write!(f, "{kind:?} limit {limit} exceeded at byte {byte_offset}"),
            Self::Conversion {
                record_index,
                byte_offset,
                source,
            } => write!(f, "record {record_index} at byte {byte_offset}: {source}"),
        }
    }
}

impl Error for ReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Xml { source, .. } => Some(source.as_ref()),
            Self::Conversion { source, .. } => Some(source),
            _ => None,
        }
    }
}
