use std::fmt;

/// Configuration, XML parsing, scalar conversion, or batch construction failure.
#[derive(Debug)]
pub enum Error {
    /// An unsupported schema, invalid path, projection, or batch size.
    Configuration(String),
    /// XML could not be read under the supported input contract.
    Xml {
        /// Byte position reached by the XML reader (not necessarily the start of the error).
        position: u64,
        /// Explanation of the failure.
        message: String,
    },
    /// A mapped field is missing, repeated, structurally invalid, or ill-typed.
    Field {
        /// Output column name.
        name: String,
        /// Relative XML path.
        path: String,
        /// Explanation of the failure.
        message: String,
    },
    /// The configured batch is full; flush before appending another record.
    BatchFull,
    /// Arrow rejected the constructed batch.
    Arrow(arrow_schema::ArrowError),
}

impl Error {
    pub(crate) fn xml(position: u64, message: impl ToString) -> Self {
        Self::Xml {
            position,
            message: message.to_string(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => write!(f, "invalid configuration: {message}"),
            Self::Xml { position, message } => write!(f, "XML at byte {position}: {message}"),
            Self::Field {
                name,
                path,
                message,
            } => write!(f, "field {name:?} at {path:?}: {message}"),
            Self::BatchFull => f.write_str("batch is full; flush before appending"),
            Self::Arrow(source) => write!(f, "Arrow: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Arrow(source) => Some(source),
            _ => None,
        }
    }
}

impl From<arrow_schema::ArrowError> for Error {
    fn from(source: arrow_schema::ArrowError) -> Self {
        Self::Arrow(source)
    }
}
