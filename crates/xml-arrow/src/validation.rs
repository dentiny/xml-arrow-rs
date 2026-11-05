use std::collections::{HashMap, HashSet};

use arrow_schema::DataType;

use crate::{Error, NamespaceBinding, Result, XmlField};

pub(crate) fn validate_fields(
    fields: &[XmlField],
    namespaces: &HashMap<String, String>,
) -> Result<()> {
    let mut names = HashSet::new();
    for field in fields {
        if field.field.name().is_empty() || !names.insert(field.field.name()) {
            return Err(Error::Configuration(
                "column names must be nonempty and unique".into(),
            ));
        }
        let valid_type = match field.field.data_type() {
            DataType::Utf8 | DataType::Boolean | DataType::Int64 | DataType::Float64 => true,
            DataType::Struct(children) => children.len() == field.children.len(),
            DataType::List(_) => field.children.len() == 1,
            _ => false,
        };
        if !valid_type {
            return Err(Error::Configuration(format!(
                "unsupported type or missing child mappings for {:?}: {}; use structure/list constructors for nested fields",
                field.field.name(), field.field.data_type()
            )));
        }
        let container = matches!(
            field.field.data_type(),
            DataType::Struct(_) | DataType::List(_)
        );
        if !valid_path(&field.path, namespaces)
            || container && field.path.split('/').any(|part| part.starts_with('@'))
        {
            return Err(Error::Configuration(format!(
                "invalid path {:?}",
                field.path
            )));
        }
        validate_fields(&field.children, namespaces)?;
    }
    Ok(())
}

pub(crate) fn validate_namespaces(
    bindings: &[NamespaceBinding<'_>],
) -> Result<HashMap<String, String>> {
    let mut namespaces = HashMap::from([("xml".into(), roxmltree::NS_XML_URI.into())]);
    for &NamespaceBinding { prefix, uri } in bindings {
        if !valid_name(prefix)
            || prefix == "xmlns"
            || uri.is_empty()
            || uri == roxmltree::NS_XMLNS_URI
            || uri == roxmltree::NS_XML_URI
            || namespaces.insert(prefix.into(), uri.into()).is_some()
        {
            return Err(Error::Configuration(format!(
                "invalid or duplicate namespace binding {prefix:?}"
            )));
        }
    }
    Ok(namespaces)
}

fn valid_path(path: &str, namespaces: &HashMap<String, String>) -> bool {
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
        match name.split_once(':') {
            Some((prefix, local)) => namespaces.contains_key(prefix) && valid_name(local),
            None => valid_name(name),
        }
    })
}

pub(crate) fn valid_name(name: &str) -> bool {
    !name.contains(':')
        && roxmltree::Document::parse(&format!("<{name}/>"))
            .is_ok_and(|doc| doc.root_element().tag_name().name() == name)
}
