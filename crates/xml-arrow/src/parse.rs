use std::collections::HashMap;

use quick_xml::{escape::unescape, events::Event, Reader, XmlVersion};

use crate::{schema::valid_name, Error, XmlField};

pub(crate) enum Scalar {
    Text(String),
    Boolean(bool),
    Integer(i64),
    Float(f64),
}

/// Keep only mapped scalar values; never construct a tree of the record.
pub(crate) fn parse(input: &str, fields: &[XmlField]) -> Result<Vec<Option<Scalar>>, Error> {
    if let Some((position, _)) = input.char_indices().find(|(_, c)| !xml_char(*c)) {
        return Err(Error::xml(
            position as u64,
            "character is not allowed in XML 1.0",
        ));
    }
    let wanted: HashMap<_, _> = fields.iter().map(|f| (f.path.as_str(), f)).collect();
    let mut values: HashMap<String, String> = HashMap::new();
    let mut reader = Reader::from_str(input);
    reader.config_mut().expand_empty_elements = true;
    reader.config_mut().check_comments = true;
    let mut stack: Vec<String> = Vec::new();
    let mut root_seen = false;
    loop {
        let start = reader.buffer_position();
        let event = reader
            .read_event()
            .map_err(|e| Error::xml(reader.error_position(), e))?;
        let position = reader.buffer_position();
        match event {
            Event::Start(element) => {
                if stack.is_empty() {
                    if root_seen {
                        return Err(Error::xml(
                            start,
                            "expected one record element, found multiple roots",
                        ));
                    }
                    root_seen = true;
                } else {
                    let parent = relative_path(&stack);
                    if let Some(field) = wanted.get(parent.as_str()) {
                        return Err(field.error("scalar text contains a child element"));
                    }
                }
                let name = std::str::from_utf8(element.name().as_ref())
                    .map_err(|e| Error::xml(position, e))?
                    .to_owned();
                check_name(&name, position)?;
                stack.push(name);
                let path = relative_path(&stack);
                if let Some(field) = wanted.get(path.as_str()) {
                    if values.insert(path.clone(), String::new()).is_some() {
                        return Err(field.error("repeated scalar element"));
                    }
                }
                // Validate every attribute, even when no columns are projected.
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(|e| Error::xml(position, e))?;
                    let key = std::str::from_utf8(attribute.key.as_ref())
                        .map_err(|e| Error::xml(position, e))?;
                    if key == "xmlns" {
                        return Err(Error::xml(position, "namespaces are not supported yet"));
                    }
                    check_name(key, position)?;
                    let value = attribute
                        .normalized_value(XmlVersion::Implicit1_0)
                        .map_err(|e| Error::xml(position, e))?;
                    check_chars(&value, position)?;
                    let attribute_path = if path == "." {
                        format!("@{key}")
                    } else {
                        format!("{path}/@{key}")
                    };
                    if let Some(field) = wanted.get(attribute_path.as_str()) {
                        if values.insert(attribute_path, value.into_owned()).is_some() {
                            return Err(field.error("repeated scalar attribute"));
                        }
                    }
                }
            }
            Event::End(_) => {
                stack.pop();
            }
            Event::Text(text) => {
                let text = text.xml10_content().map_err(|e| Error::xml(position, e))?;
                if stack.is_empty() && !text.chars().all(xml_space) {
                    return Err(Error::xml(start, "text outside the record element"));
                }
                if !stack.is_empty() {
                    if let Some(value) = values.get_mut(&relative_path(&stack)) {
                        value.push_str(&text);
                    }
                }
            }
            Event::CData(text) => {
                if stack.is_empty() {
                    return Err(Error::xml(start, "CDATA outside the record element"));
                }
                let text = text.xml10_content().map_err(|e| Error::xml(position, e))?;
                if let Some(value) = values.get_mut(&relative_path(&stack)) {
                    value.push_str(&text);
                }
            }
            Event::GeneralRef(reference) => {
                if stack.is_empty() {
                    return Err(Error::xml(start, "reference outside the record element"));
                }
                let name = reference.decode().map_err(|e| Error::xml(position, e))?;
                let escaped = format!("&{name};");
                let text = unescape(&escaped).map_err(|e| Error::xml(position, e))?;
                check_chars(&text, position)?;
                if let Some(value) = values.get_mut(&relative_path(&stack)) {
                    value.push_str(&text);
                }
            }
            Event::Decl(declaration) => {
                if start != 0 && !(start == 3 && input.starts_with('\u{FEFF}')) {
                    return Err(Error::xml(
                        start,
                        "XML declaration must be at the beginning",
                    ));
                }
                if declaration
                    .version()
                    .map_err(|e| Error::xml(position, e))?
                    .as_ref()
                    != b"1.0"
                {
                    return Err(Error::xml(position, "only XML 1.0 is supported"));
                }
                if let Some(encoding) = declaration.encoding() {
                    let encoding = encoding.map_err(|e| Error::xml(position, e))?;
                    if !encoding.eq_ignore_ascii_case(b"utf-8") {
                        return Err(Error::xml(position, "append_xml accepts UTF-8 only"));
                    }
                }
            }
            Event::DocType(_) => {
                return Err(Error::xml(
                    start,
                    "DTDs and custom entities are not supported",
                ))
            }
            Event::Eof => break,
            Event::Comment(_) | Event::PI(_) => {}
            Event::Empty(_) => unreachable!("empty elements are expanded by reader configuration"),
        }
    }
    if !root_seen || !stack.is_empty() {
        return Err(Error::xml(
            reader.buffer_position(),
            "expected one complete record element",
        ));
    }
    fields
        .iter()
        .map(|field| convert(field, values.get(&field.path)))
        .collect()
}

fn relative_path(stack: &[String]) -> String {
    if stack.len() <= 1 {
        ".".into()
    } else {
        stack[1..].join("/")
    }
}

fn check_name(name: &str, position: u64) -> Result<(), Error> {
    if !valid_name(name) {
        return Err(Error::xml(
            position,
            format!("invalid or namespace-qualified name {name:?}"),
        ));
    }
    Ok(())
}

fn xml_char(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

fn check_chars(text: &str, position: u64) -> Result<(), Error> {
    if text.chars().all(xml_char) {
        Ok(())
    } else {
        Err(Error::xml(
            position,
            "reference contains a character not allowed in XML 1.0",
        ))
    }
}

fn xml_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

fn convert(field: &XmlField, value: Option<&String>) -> Result<Option<Scalar>, Error> {
    use arrow_schema::DataType;
    let Some(value) = value else {
        return if field.field.is_nullable() {
            Ok(None)
        } else {
            Err(field.error("required value is missing"))
        };
    };
    let trimmed = value.trim_matches(xml_space);
    let invalid = || {
        field.error(format!(
            "expected {}, got {value:?}",
            field.field.data_type()
        ))
    };
    let scalar = match field.field.data_type() {
        DataType::Utf8 => Scalar::Text(value.clone()),
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
