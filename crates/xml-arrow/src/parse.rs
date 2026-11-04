use roxmltree::{Document, Node};

use crate::{Error, XmlField};

pub(crate) enum Scalar {
    Text(String),
    Boolean(bool),
    Integer(i64),
    Float(f64),
}

pub(crate) fn parse(input: &str, fields: &[XmlField]) -> Result<Vec<Option<Scalar>>, Error> {
    let document = Document::parse(input)?;
    if document.descendants().any(|node| {
        node.is_element()
            && (node.namespaces().len() != 0
                || node.tag_name().namespace().is_some()
                || node.attributes().any(|attr| attr.namespace().is_some()))
    }) {
        return Err(Error::UnsupportedXml(
            "namespace mapping is not supported yet",
        ));
    }
    fields
        .iter()
        .map(|field| {
            let value = extract(document.root_element(), field)?;
            convert(field, value.as_deref())
        })
        .collect()
}

fn extract(root: Node<'_, '_>, field: &XmlField) -> Result<Option<String>, Error> {
    let mut nodes = vec![root];
    for part in field.path.split('/') {
        if part == "." {
            continue;
        }
        if let Some(attribute) = part.strip_prefix('@') {
            let mut values = nodes.iter().filter_map(|node| node.attribute(attribute));
            let value = values.next().map(str::to_owned);
            if values.next().is_some() {
                return Err(field.error("repeated scalar attribute"));
            }
            return Ok(value);
        }
        nodes = nodes
            .into_iter()
            .flat_map(|node| node.children())
            .filter(|node| node.has_tag_name(part))
            .collect();
    }
    if nodes.len() > 1 {
        return Err(field.error("repeated scalar element"));
    }
    let Some(node) = nodes.first() else {
        return Ok(None);
    };
    if node.children().any(|child| child.is_element()) {
        return Err(field.error("scalar text contains a child element"));
    }
    Ok(Some(
        node.children()
            .filter(|child| child.is_text())
            .filter_map(|child| child.text())
            .collect(),
    ))
}

fn convert(field: &XmlField, value: Option<&str>) -> Result<Option<Scalar>, Error> {
    use arrow_schema::DataType;
    let Some(value) = value else {
        return if field.field.is_nullable() {
            Ok(None)
        } else {
            Err(field.error("required value is missing"))
        };
    };
    let trimmed = value.trim_matches([' ', '\t', '\r', '\n']);
    let invalid = || {
        field.error(format!(
            "expected {}, got {value:?}",
            field.field.data_type()
        ))
    };
    let scalar = match field.field.data_type() {
        DataType::Utf8 => Scalar::Text(value.to_owned()),
        DataType::Boolean => Scalar::Boolean(match trimmed {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => return Err(invalid()),
        }),
        DataType::Int64 => Scalar::Integer(trimmed.parse().map_err(|_| invalid())?),
        DataType::Float64 => {
            let number: f64 = trimmed.parse().map_err(|_| invalid())?;
            if !number.is_finite() {
                return Err(invalid());
            }
            Scalar::Float(number)
        }
        _ => unreachable!("schema types are validated on construction"),
    };
    Ok(Some(scalar))
}
