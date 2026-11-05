#![deny(missing_docs)]
//! Reopenable local and OpenDAL storage inputs. XML parsing stays in `xml-arrow`.

mod file;

pub use file::XmlFile;
pub use opendal;
