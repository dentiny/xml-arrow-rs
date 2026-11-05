use std::{borrow::Cow, io, path::Path};

use opendal::{services::Fs, FuturesAsyncReader, Operator};
use url::Url;

fn invalid(message: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.to_string())
}

/// A reusable description of one input object. Constructors do not open it.
///
/// ```
/// # fn main() -> std::io::Result<()> {
/// let file = xml_io::XmlFile::from_uri("records.xml")?;
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
        if path.is_empty() || path.ends_with('/') {
            return Err(invalid("input must name an object"));
        }
        Ok(Self {
            operator: Some(operator),
            path,
        })
    }

    /// Describes a local path or file URL without accessing the file.
    /// Relative paths are resolved against the current directory now, so later
    /// opens use the same location. Other storage schemes require [`Self::new`].
    pub fn from_uri(uri: &str) -> io::Result<Self> {
        if uri.is_empty() {
            return Err(invalid("input URI must not be empty"));
        }
        let path = if Path::new(uri).is_absolute() {
            uri.into()
        } else {
            match Url::parse(uri) {
                Ok(url) if url.scheme() == "file" => url
                    .to_file_path()
                    .map_err(|_| invalid("invalid file URL"))?,
                _ if uri.contains("://") => {
                    return Err(invalid(
                        "use XmlFile::new with a configured operator for this storage service",
                    ))
                }
                _ => uri.into(),
            }
        };
        let path = std::path::absolute(path)?;
        if path.file_name().is_none() {
            return Err(invalid("input must name a file"));
        }
        let path = path
            .into_os_string()
            .into_string()
            .map_err(|_| invalid("local path must be UTF-8"))?;
        Ok(Self {
            operator: None,
            path,
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
                let absolute = Path::new(&self.path);
                // Use the filesystem root, so OpenDAL cannot create a missing
                // input parent directory while building its filesystem backend.
                let root = absolute
                    .ancestors()
                    .last()
                    .ok_or_else(|| invalid("missing filesystem root"))?;
                let path = absolute.strip_prefix(root).map_err(invalid)?;
                let path = path
                    .to_str()
                    .ok_or_else(|| invalid("local path must be UTF-8"))?;
                let operator = Operator::new(
                    Fs::default().root(
                        root.to_str()
                            .ok_or_else(|| invalid("local root must be UTF-8"))?,
                    ),
                )?;
                #[cfg(windows)]
                let path = Cow::Owned(path.replace('\\', "/"));
                #[cfg(not(windows))]
                let path = Cow::Borrowed(path);
                (operator, path)
            }
        };
        Ok(operator
            .reader(&path)
            .await?
            .into_futures_async_read(..)
            .await?)
    }

    /// Opens and incrementally decompresses GZIP bytes, including concatenated
    /// members. Requires the `gzip` feature. Compression is selected explicitly;
    /// [`Self::open`] always returns the original bytes regardless of filename.
    /// Downstream XML reader offsets count decompressed bytes.
    #[cfg(feature = "gzip")]
    pub async fn open_gzip(
        &self,
    ) -> io::Result<impl futures_util::io::AsyncBufRead + Unpin + Send> {
        let mut decoder = async_compression::futures::bufread::GzipDecoder::new(self.open().await?);
        decoder.multiple_members(true);
        Ok(futures_util::io::BufReader::new(decoder))
    }
}
