use std::fmt;

/// Result with [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// XML-to-Arrow conversion error.
#[derive(Debug)]
pub enum Error {
    /// Invalid schema, path, or options.
    Configuration(String),
    /// XML parsing error.
    Xml(roxmltree::Error),
    /// Unsupported XML feature.
    UnsupportedXml(&'static str),
    /// Invalid or missing field value.
    Field {
        /// Column name.
        name: String,
        /// Relative XML path.
        path: String,
        /// Failure reason.
        message: String,
    },
    /// Flush before appending more rows.
    BatchFull,
    /// Arrow error.
    Arrow(arrow_schema::ArrowError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => write!(f, "invalid configuration: {message}"),
            Self::Xml(source) => write!(f, "XML: {source}"),
            Self::UnsupportedXml(message) => write!(f, "unsupported XML: {message}"),
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
