//! Reopenable file and object inputs.

use std::{borrow::Cow, io};

use futures_util::io::AsyncBufRead;
use opendal::{services::Fs, Operator};

use crate::validation::{validate_local_path, validate_local_path_parts, validate_object_path};

/// A reopenable storage input.
///
/// ```
/// # fn main() -> std::io::Result<()> {
/// let file = xml_io::file::XmlFile::from_uri("records.xml")?;
/// assert!(file.source_uri().ends_with("records.xml"));
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct XmlFile {
    // None uses the local filesystem.
    operator: Option<Operator>,
    path: String,
}

impl XmlFile {
    /// Describes an object in a configured OpenDAL operator.
    pub fn new(operator: Operator, path: impl Into<String>) -> io::Result<Self> {
        let path = path.into();
        validate_object_path(&path)?;
        Ok(Self {
            operator: Some(operator),
            path,
        })
    }

    /// Resolves a local path or file URL without opening it.
    /// Relative paths use the current directory.
    pub fn from_uri(uri: &str) -> io::Result<Self> {
        Ok(Self {
            operator: None,
            path: validate_local_path(uri)?,
        })
    }

    /// Absolute local path or configured object key.
    pub fn source_uri(&self) -> &str {
        &self.path
    }

    /// Opens a new byte stream.
    pub async fn open(&self) -> io::Result<impl AsyncBufRead + Unpin + Send> {
        let (operator, path) = match &self.operator {
            Some(operator) => (operator.clone(), Cow::Borrowed(self.path.as_str())),
            None => {
                let (root, path) = validate_local_path_parts(&self.path).await?;
                // Avoid creating input directories.
                let operator = Operator::new(Fs::default().root(&root))?;
                (operator, Cow::Owned(path))
            }
        };
        Ok(operator
            .reader(&path)
            .await?
            .into_futures_async_read(..)
            .await?)
    }
}
