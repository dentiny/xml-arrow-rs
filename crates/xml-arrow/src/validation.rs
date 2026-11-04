use std::collections::{HashMap, HashSet};

use arrow_array::builder::{ArrayBuilder, ListBuilder, StringBuilder, StructBuilder};
use arrow_schema::DataType;

use crate::{parse::Value, Error, Result, XmlField};

// All items in a row share each nested Arrow builder. Check their cumulative
// offsets together before append, including for lists of lists or structs.
pub(crate) fn validate_capacity(
    builder: &dyn ArrayBuilder,
    field: &XmlField,
    values: &[&Value],
) -> Result<()> {
    match field.field.data_type() {
        DataType::Utf8 => {
            let builder = builder.as_any().downcast_ref::<StringBuilder>().unwrap();
            let bytes = values
                .iter()
                .filter_map(|value| match value {
                    Value::Text(text) => Some(text.len()),
                    _ => None,
                })
                .fold(0usize, usize::saturating_add);
            if builder.values_slice().len().saturating_add(bytes) > i32::MAX as usize {
                return Err(
                    field.error("Utf8 offset limit exceeded; use a smaller batch or record")
                );
            }
        }
        DataType::Struct(_) => {
            let builder = builder.as_any().downcast_ref::<StructBuilder>().unwrap();
            for (index, (builder, child)) in builder
                .field_builders()
                .iter()
                .zip(&field.children)
                .enumerate()
            {
                let children: Vec<_> = values
                    .iter()
                    .filter_map(|value| match value {
                        Value::Struct(children) => Some(&children[index]),
                        _ => None,
                    })
                    .collect();
                validate_capacity(builder.as_ref(), child, &children)?;
            }
        }
        DataType::List(_) => {
            let builder = builder
                .as_any()
                .downcast_ref::<ListBuilder<Box<dyn ArrayBuilder>>>()
                .unwrap();
            let children: Vec<_> = values
                .iter()
                .filter_map(|value| match value {
                    Value::List(children) => Some(children),
                    _ => None,
                })
                .flatten()
                .collect();
            if builder.values_ref().len().saturating_add(children.len()) > i32::MAX as usize {
                return Err(
                    field.error("List offset limit exceeded; use a smaller batch or record")
                );
            }
            validate_capacity(builder.values_ref().as_ref(), &field.children[0], &children)?;
        }
        _ => {}
    }
    Ok(())
}

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

pub(crate) fn validate_namespaces(bindings: &[(&str, &str)]) -> Result<HashMap<String, String>> {
    let mut namespaces = HashMap::from([("xml".into(), roxmltree::NS_XML_URI.into())]);
    for &(prefix, uri) in bindings {
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

fn valid_name(name: &str) -> bool {
    !name.contains(':')
        && roxmltree::Document::parse(&format!("<{name}/>"))
            .is_ok_and(|doc| doc.root_element().tag_name().name() == name)
}
