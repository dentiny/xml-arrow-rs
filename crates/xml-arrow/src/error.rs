use std::fmt;

/// A result using this crate's [`Error`] type.
pub type Result<T> = std::result::Result<T, Error>;

/// Configuration, XML parsing, scalar conversion, or batch construction failure.
#[derive(Debug)]
pub enum Error {
    /// An unsupported schema, invalid path, projection, or batch size.
    Configuration(String),
    /// XML syntax error, including the parser's source location.
    Xml(roxmltree::Error),
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

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => write!(f, "invalid configuration: {message}"),
            Self::Xml(source) => write!(f, "XML: {source}"),
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
            Self::Xml(source) => Some(source),
            Self::Arrow(source) => Some(source),
            _ => None,
        }
    }
}

impl From<roxmltree::Error> for Error {
    fn from(source: roxmltree::Error) -> Self {
        Self::Xml(source)
    }
}

impl From<arrow_schema::ArrowError> for Error {
    fn from(source: arrow_schema::ArrowError) -> Self {
        Self::Arrow(source)
    }
}
