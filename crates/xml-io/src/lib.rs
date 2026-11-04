#![deny(missing_docs)]
//! Asynchronous XML document readers with explicit record paths and byte limits.
//!
//! The public input trait is `futures::io::AsyncBufRead`. No Tokio runtime is
//! needed; a compatibility adapter connects it to quick-xml's async parser.
//!
//! ```
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use arrow_schema::DataType;
//! use futures_util::io::Cursor;
//! use xml_arrow::{XmlField, XmlSchema};
//! use xml_io::{ReaderOptions, XmlReader};
//! let schema = XmlSchema::new(vec![
//!     XmlField::new("id", "@id", DataType::Int64, false),
//! ])?;
//! let mut reader = XmlReader::new(
//!     Cursor::new(b"<rows><row id='1'/><row id='2'/></rows>"),
//!     schema, "/rows/row", ReaderOptions::default(),
//! )?;
//! futures_executor::block_on(async {
//!     assert_eq!(reader.next_batch().await?.unwrap().num_rows(), 2);
//!     assert!(reader.next_batch().await?.is_none());
//!     xml_io::Result::Ok(())
//! })?;
//! # Ok(())
//! # }
//! ```

mod error;
mod framing;
mod options;
mod reader;

pub use error::{Limit, ReadError, Result};
pub use options::ReaderOptions;
pub use reader::XmlReader;
