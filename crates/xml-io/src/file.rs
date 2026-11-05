//! Reopenable file and object inputs.

use std::{borrow::Cow, io};

use opendal::{services::Fs, FuturesAsyncReader, Operator};

use crate::validation::{local_path_parts, validate_local_path, validate_object_path};

/// A reusable description of one input object. Constructors do not open it.
///
/// ```
/// # fn main() -> std::io::Result<()> {
/// let file = xml_io::file::XmlFile::from_uri("records.xml")?;
/// // No file is opened until file.open().await is called.
/// assert!(file.source_uri().ends_with("records.xml"));
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct XmlFile {
    // None selects the local filesystem; configured operators are kept unchanged.
    operator: Option<Operator>,
    // Absolute local path or operator-relative object key; also the source label.
    path: String,
}

impl XmlFile {
    /// Describes an object in a caller-configured operator, preserving its root,
    /// credentials, transport, and layers. No global defaults are installed.
    pub fn new(operator: Operator, path: impl Into<String>) -> io::Result<Self> {
        let path = path.into();
        validate_object_path(&path)?;
        Ok(Self {
            operator: Some(operator),
            path,
        })
    }

    /// Describes a local path or file URL without accessing the file.
    /// Relative paths are resolved against the current directory now, so later
    /// opens use the same location. Other storage schemes require [`Self::new`].
    pub fn from_uri(uri: &str) -> io::Result<Self> {
        Ok(Self {
            operator: None,
            path: validate_local_path(uri)?,
        })
    }

    /// Source label for associating data or errors with this input: the absolute
    /// local path, or the object key supplied to [`Self::new`].
    pub fn source_uri(&self) -> &str {
        &self.path
    }

    /// Opens an independent stream of the original bytes, without buffering the
    /// object in full. Missing objects may fail here or on the first read.
    /// Runtime requirements follow the backend; local files require Tokio.
    pub async fn open(&self) -> io::Result<FuturesAsyncReader> {
        let (operator, path) = match &self.operator {
            Some(operator) => (operator.clone(), Cow::Borrowed(self.path.as_str())),
            None => {
                let (root, path) = local_path_parts(&self.path)?;
                // The filesystem root avoids creating a missing input parent.
                let operator = Operator::new(Fs::default().root(root))?;
                (operator, path)
            }
        };
        Ok(operator
            .reader(&path)
            .await?
            .into_futures_async_read(..)
            .await?)
    }
}
