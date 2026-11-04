#![deny(missing_docs)]
//! Convert individual XML records to Apache Arrow with an explicit schema.
//!
//! Each call to [`XmlBatchBuilder::append_xml`] supplies one complete XML
//! element. Paths are relative to that element. This crate performs no I/O.
//!
//! ```
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
//! # Ok::<(), xml_arrow::Error>(())
//! ```

mod batch;
mod error;
mod parse;
mod schema;

pub use batch::XmlBatchBuilder;
pub use error::Error;
pub use schema::{XmlField, XmlSchema};
