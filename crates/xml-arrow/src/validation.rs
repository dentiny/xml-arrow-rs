use std::collections::HashSet;

use arrow_schema::DataType;

use crate::{Error, Result, XmlField};

pub(crate) fn validate_fields(fields: &[XmlField]) -> Result<()> {
    let mut names = HashSet::new();
    for field in fields {
        if field.field.name().is_empty() || !names.insert(field.field.name()) {
            return Err(Error::Configuration(
                "column names must be nonempty and unique".into(),
            ));
        }
        if !matches!(
            field.field.data_type(),
            DataType::Utf8 | DataType::Boolean | DataType::Int64 | DataType::Float64
        ) {
            return Err(Error::Configuration(format!(
                "unsupported type for {:?}: {}",
                field.field.name(),
                field.field.data_type()
            )));
        }
        if !valid_path(&field.path) {
            return Err(Error::Configuration(format!(
                "invalid path {:?}",
                field.path
            )));
        }
    }
    Ok(())
}

fn valid_path(path: &str) -> bool {
    if path == "." {
        return true;
    }
    let parts: Vec<_> = path.split('/').collect();
    parts.iter().enumerate().all(|(index, part)| {
        let name = if let Some(attribute) = part.strip_prefix('@') {
            if index + 1 != parts.len() {
                return false;
            }
            attribute
        } else {
            part
        };
        valid_name(name)
    })
}

fn valid_name(name: &str) -> bool {
    !name.contains(':')
        && roxmltree::Document::parse(&format!("<{name}/>"))
            .is_ok_and(|doc| doc.root_element().tag_name().name() == name)
}
