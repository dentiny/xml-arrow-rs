#![deny(missing_docs)]
//! Convert XML records to Arrow batches with explicit or inferred schemas.
//!
//! Use [`XmlBatchBuilder`] for record elements and [`XmlReader`] for documents.
//! [`XmlReader::infer`] samples a document to infer its schema.
//!
//! ```
//! # fn main() -> xml_arrow::Result<()> {
//! use arrow_schema::DataType;
//! use xml_arrow::{XmlBatchBuilder, XmlField, XmlSchema};
//!
//! let schema = XmlSchema::new(vec![
//!     XmlField::new("id", "@id", DataType::Int64, false),
//!     XmlField::new("title", "title", DataType::Utf8, true),
//! ])?;
//! let mut builder = XmlBatchBuilder::new(schema, 128)?;
//! builder.append_xml(r#"<book id="7"><title>Rust</title></book>"#)?;
//! let batch = builder.flush()?.unwrap();
//! assert_eq!(batch.num_rows(), 1);
//! assert_eq!(batch.num_columns(), 2);
//! # Ok(())
//! # }
//! ```
//!
//! Nested fields:
//!
//! ```
//! # fn main() -> xml_arrow::Result<()> {
//! use arrow_schema::DataType;
//! use xml_arrow::{XmlBatchBuilder, XmlField, XmlSchema};
//! let schema = XmlSchema::new(vec![XmlField::list(
//!     "people", "people", false,
//!     XmlField::structure("item", "person", false, vec![
//!         XmlField::new("id", "@id", DataType::Int64, false),
//!     ]),
//! )])?;
//! let mut builder = XmlBatchBuilder::new(schema, 128)?;
//! builder.append_xml(r#"<r><people><person id="1"/><person id="2"/></people></r>"#)?;
//! assert_eq!(builder.flush()?.unwrap().num_rows(), 1);
//! # Ok(())
//! # }
//! ```

//! Read a document:
//!
//! ```
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use arrow_schema::DataType;
//! use futures_util::io::Cursor;
//! use xml_arrow::{ReaderOptions, XmlField, XmlReader, XmlSchema};
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
//!     xml_arrow::ReaderResult::Ok(())
//! })?;
//! # Ok(())
//! # }
//! ```

mod batch;
mod builders;
mod error;
mod parse;
mod reader;
mod schema;
mod validation;

pub use batch::XmlBatchBuilder;
pub use error::{Error, Result};
pub use schema::{NamespaceBinding, XmlField, XmlSchema};

pub use reader::{
    read_records, InferenceOptions, Limit, ReadError, ReaderOptions, ReaderResult, XmlReader,
    XmlRecord,
};
