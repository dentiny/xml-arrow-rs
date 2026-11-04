#![deny(missing_docs)]
//! Convert individual XML records to Apache Arrow with an explicit schema.
//!
//! Each call to [`XmlBatchBuilder::append_xml`] supplies one complete XML
//! element. Paths are relative to that element. This crate performs no I/O.
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
//! Nested child paths are relative to their selected parent element:
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

mod batch;
mod builders;
mod error;
mod parse;
mod schema;
mod validation;

pub use batch::XmlBatchBuilder;
pub use error::{Error, Result};
pub use schema::{XmlField, XmlSchema};
