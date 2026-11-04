use std::collections::HashMap;

use roxmltree::{Document, Node};

use crate::{Result, XmlField, XmlSchema};

pub(crate) enum Value {
    Null,
    Text(String),
    Boolean(bool),
    Integer(i64),
    Float(f64),
    Struct(Vec<Value>),
    List(Vec<Value>),
}

#[derive(Clone, Copy)]
enum Source<'a, 'input> {
    Element(Node<'a, 'input>),
    Attribute(&'a str),
}

pub(crate) fn parse(input: &str, schema: &XmlSchema) -> Result<Vec<Value>> {
    let document = Document::parse(input)?;
    schema
        .fields
        .iter()
        .map(|field| extract(document.root_element(), field, &schema.namespaces))
        .collect()
}

fn extract(
    root: Node<'_, '_>,
    field: &XmlField,
    namespaces: &HashMap<String, String>,
) -> Result<Value> {
    match select(root, &field.path, namespaces).as_slice() {
        [] => null_value(field),
        [source] => read(*source, field, namespaces),
        _ => Err(field.error("repeated value; use a list item mapping")),
    }
}

fn read(
    source: Source<'_, '_>,
    field: &XmlField,
    namespaces: &HashMap<String, String>,
) -> Result<Value> {
    use arrow_schema::DataType;
    let node = match source {
        Source::Element(node) => node,
        Source::Attribute(value) => return convert(field, value),
    };
    if let Some(nil) = node.attribute(("http://www.w3.org/2001/XMLSchema-instance", "nil")) {
        match nil.trim_matches([' ', '\t', '\r', '\n']) {
            "true" | "1" => {
                if node
                    .children()
                    .any(|child| child.is_element() || has_text(child))
                {
                    return Err(
                        field.error("xsi:nil element must not contain text or child elements")
                    );
                }
                return null_value(field);
            }
            "false" | "0" => {}
            _ => return Err(field.error("xsi:nil must be true, false, 1, or 0")),
        }
    }
    let value = match field.field.data_type() {
        DataType::Struct(_) => {
            reject_mixed_content(node, field)?;
            Value::Struct(
                field
                    .children
                    .iter()
                    .map(|child| extract(node, child, namespaces))
                    .collect::<Result<_>>()?,
            )
        }
        DataType::List(_) => {
            reject_mixed_content(node, field)?;
            let item = &field.children[0];
            Value::List(
                select(node, &item.path, namespaces)
                    .into_iter()
                    .map(|source| read(source, item, namespaces))
                    .collect::<Result<_>>()?,
            )
        }
        _ => {
            if node.children().any(|child| child.is_element()) {
                return Err(field.error("scalar text contains a child element"));
            }
            let text: String = node
                .children()
                .filter(|child| child.is_text())
                .filter_map(|child| child.text())
                .collect();
            convert(field, &text)?
        }
    };
    Ok(value)
}

fn select<'a, 'input>(
    root: Node<'a, 'input>,
    path: &str,
    namespaces: &HashMap<String, String>,
) -> Vec<Source<'a, 'input>> {
    let mut nodes = vec![root];
    for part in path.split('/') {
        if part == "." {
            continue;
        }
        let attribute = part.strip_prefix('@');
        let name = attribute.unwrap_or(part);
        let (uri, local) = match name.split_once(':') {
            Some((prefix, local)) => (Some(namespaces[prefix].as_str()), local),
            None => (None, name),
        };
        if attribute.is_some() {
            return nodes
                .iter()
                .filter_map(|node| {
                    node.attributes()
                        .find(|attr| {
                            attr.namespace().filter(|uri| !uri.is_empty()) == uri
                                && attr.name() == local
                        })
                        .map(|attr| Source::Attribute(attr.value()))
                })
                .collect();
        }
        nodes = nodes
            .into_iter()
            .flat_map(|node| node.children())
            .filter(|node| {
                node.is_element()
                    && node.tag_name().namespace().filter(|uri| !uri.is_empty()) == uri
                    && node.tag_name().name() == local
            })
            .collect();
    }
    nodes.into_iter().map(Source::Element).collect()
}

fn null_value(field: &XmlField) -> Result<Value> {
    if field.field.is_nullable() {
        Ok(Value::Null)
    } else {
        Err(field.error("required value is missing or nil"))
    }
}

fn has_text(node: Node<'_, '_>) -> bool {
    node.is_text()
        && !node
            .text()
            .unwrap_or_default()
            .trim_matches([' ', '\t', '\r', '\n'])
            .is_empty()
}

fn reject_mixed_content(node: Node<'_, '_>, field: &XmlField) -> Result<()> {
    if node.children().any(|child| child.is_element()) && node.children().any(has_text) {
        return Err(
            field.error("mixed text and child elements cannot be mapped to a struct or list")
        );
    }
    Ok(())
}

fn convert(field: &XmlField, value: &str) -> Result<Value> {
    use arrow_schema::DataType;
    let trimmed = value.trim_matches([' ', '\t', '\r', '\n']);
    let invalid = || {
        field.error(format!(
            "expected {}, got {value:?}",
            field.field.data_type()
        ))
    };
    Ok(match field.field.data_type() {
        DataType::Utf8 => Value::Text(value.to_owned()),
        DataType::Boolean => Value::Boolean(match trimmed {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => return Err(invalid()),
        }),
        DataType::Int64 => Value::Integer(trimmed.parse().map_err(|_| invalid())?),
        DataType::Float64 => {
            let number: f64 = trimmed.parse().map_err(|_| invalid())?;
            if !number.is_finite() {
                return Err(invalid());
            }
            Value::Float(number)
        }
        _ => unreachable!("nested types are handled before scalar conversion"),
    })
}
