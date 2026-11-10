/// Result with [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// XML-to-Arrow conversion error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Invalid schema, path, or options.
    #[error("invalid configuration: {0}")]
    Configuration(String),
    /// XML parsing error.
    #[error("XML: {0}")]
    Xml(#[from] roxmltree::Error),
    /// Unsupported XML feature.
    #[error("unsupported XML: {0}")]
    UnsupportedXml(&'static str),
    /// Invalid or missing field value.
    #[error("field {name:?} at {path:?}: {message}")]
    Field {
        /// Column name.
        name: String,
        /// Relative XML path.
        path: String,
        /// Failure reason.
        message: String,
    },
    /// Flush before appending more rows.
    #[error("batch is full; flush before appending")]
    BatchFull,
    /// Arrow error.
    #[error("Arrow: {0}")]
    Arrow(#[from] arrow_schema::ArrowError),
}
