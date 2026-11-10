use std::collections::{BTreeMap, BTreeSet};

use arrow_schema::DataType;
use roxmltree::Node;

use crate::{
    parse::{is_nil, trim_xml_whitespace},
    Error, NamespaceBinding, Result, XmlField, XmlSchema,
};

// (Optional namespace URI, local name); attributes use "@name", text uses ".".
type Name = (Option<String>, String);
// Key: XML name. Value: inferred type and repetition.
type Fields = BTreeMap<Name, Shape>;

#[derive(Default)]
enum Kind {
    #[default]
    Unknown,
    Empty,
    Scalar {
        data_type: DataType,
        // Whether every sampled integer can be represented exactly as Float64.
        exact_float: bool,
    },
    Object(Fields),
}

struct Shape {
    kind: Kind,
    repeated: bool,
}

#[derive(Default)]
pub(super) struct Inference(Kind);

impl Inference {
    pub(super) fn observe(&mut self, xml: &str) -> Result<()> {
        let document = roxmltree::Document::parse(xml)?;
        self.0.merge(element(document.root_element(), true)?)
    }

    pub(super) fn finish(self, bindings: &[NamespaceBinding<'_>]) -> Result<XmlSchema> {
        let mut uris = BTreeSet::new();
        self.0.namespaces(&mut uris);
        let mut namespaces = BTreeMap::from([(roxmltree::NS_XML_URI.to_owned(), "xml".to_owned())]);
        for &NamespaceBinding { prefix, uri } in bindings {
            let previous = namespaces
                .entry(uri.to_owned())
                .or_insert_with(|| prefix.to_owned());
            if prefix < previous.as_str() {
                *previous = prefix.to_owned();
            }
        }
        let mut next = 0;
        for uri in uris {
            if namespaces.contains_key(&uri) {
                continue;
            }
            let prefix = loop {
                let prefix = format!("ns{next}");
                next += 1;
                if !namespaces.values().any(|value| value == &prefix) {
                    break prefix;
                }
            };
            namespaces.insert(uri, prefix);
        }
        let fields = match self.0 {
            Kind::Object(fields) => fields,
            kind => BTreeMap::from([(
                (None, ".".into()),
                Shape {
                    kind,
                    repeated: false,
                },
            )]),
        };
        let fields = fields
            .into_iter()
            .map(|(name, shape)| shape.field(name, &namespaces))
            .collect();
        let bindings: Vec<_> = namespaces
            .iter()
            .filter(|(uri, _)| uri.as_str() != roxmltree::NS_XML_URI)
            .map(|(uri, prefix)| NamespaceBinding {
                prefix: prefix.as_str(),
                uri: uri.as_str(),
            })
            .collect();
        XmlSchema::with_namespaces(fields, &bindings)
    }
}

impl Kind {
    fn merge(&mut self, other: Self) -> Result<()> {
        match (&mut *self, other) {
            (_, Self::Unknown) => {}
            (kind @ Self::Unknown, other) => *kind = other,
            (Self::Empty, Self::Empty) => {}
            (kind @ Self::Empty, Self::Object(fields)) => *kind = Self::Object(fields),
            (Self::Object(_), Self::Empty) => {}
            (kind @ Self::Empty, Self::Scalar { .. })
            | (kind @ Self::Scalar { .. }, Self::Empty) => {
                *kind = Self::Scalar {
                    data_type: DataType::Utf8,
                    exact_float: true,
                }
            }
            (
                Self::Scalar {
                    data_type: left,
                    exact_float: left_exact,
                },
                Self::Scalar {
                    data_type: right,
                    exact_float: right_exact,
                },
            ) => {
                *left_exact &= right_exact;
                if *left != right {
                    *left = if *left_exact
                        && matches!(
                            (&*left, right),
                            (DataType::Int64, DataType::Float64)
                                | (DataType::Float64, DataType::Int64)
                        ) {
                        DataType::Float64
                    } else {
                        DataType::Utf8
                    };
                }
            }
            (Self::Object(left), Self::Object(right)) => {
                for (name, shape) in right {
                    if let Some(previous) = left.get_mut(&name) {
                        previous.kind.merge(shape.kind)?;
                        previous.repeated |= shape.repeated;
                    } else {
                        left.insert(name, shape);
                    }
                }
                if left.contains_key(&(None, ".".into()))
                    && left
                        .keys()
                        .any(|(_, name)| name != "." && !name.starts_with('@'))
                {
                    return Err(Error::UnsupportedXml(
                        "cannot infer both text and child-element mappings",
                    ));
                }
            }
            _ => {
                return Err(Error::UnsupportedXml(
                    "cannot infer one field from both scalar and structured elements",
                ))
            }
        }
        Ok(())
    }

    fn namespaces(&self, uris: &mut BTreeSet<String>) {
        if let Self::Object(fields) = self {
            for ((uri, _), shape) in fields {
                if let Some(uri) = uri {
                    uris.insert(uri.clone());
                }
                shape.kind.namespaces(uris);
            }
        }
    }
}

impl Shape {
    fn field(self, name: Name, namespaces: &BTreeMap<String, String>) -> XmlField {
        if self.repeated {
            let item = Self {
                repeated: false,
                ..self
            }
            .field(name, namespaces);
            let name = item.arrow_field().name().clone();
            return XmlField::list(name, ".", true, item);
        }
        let (uri, local) = name;
        let path = match uri {
            None => local,
            Some(uri) => match local.strip_prefix('@') {
                Some(local) => format!("@{}:{local}", namespaces[&uri]),
                None => format!("{}:{local}", namespaces[&uri]),
            },
        };
        match self.kind {
            Kind::Unknown | Kind::Empty => XmlField::new(&path, &path, DataType::Utf8, true),
            Kind::Scalar { data_type, .. } => XmlField::new(&path, &path, data_type, true),
            Kind::Object(fields) => {
                let children = fields
                    .into_iter()
                    .map(|(name, shape)| shape.field(name, namespaces))
                    .collect();
                XmlField::structure(&path, &path, true, children)
            }
        }
    }
}

fn element(node: Node<'_, '_>, record: bool) -> Result<Kind> {
    // Record attributes map directly to columns, including on nil records.
    if is_nil(node).map_err(Error::UnsupportedXml)? && !record {
        return Ok(Kind::Unknown);
    }
    let mut fields = Fields::new();
    for attribute in node.attributes() {
        if attribute.namespace() == Some("http://www.w3.org/2001/XMLSchema-instance")
            && attribute.name() == "nil"
        {
            continue;
        }
        fields.insert(
            (
                attribute.namespace().map(str::to_owned),
                format!("@{}", attribute.name()),
            ),
            Shape {
                kind: scalar(attribute.value()),
                repeated: false,
            },
        );
    }
    let mut children = false;
    let mut text = String::new();
    for child in node.children() {
        if child.is_element() {
            children = true;
            let name = child.tag_name();
            let name = (name.namespace().map(str::to_owned), name.name().to_owned());
            let kind = element(child, false)?;
            if let Some(previous) = fields.get_mut(&name) {
                previous.kind.merge(kind)?;
                previous.repeated = true;
            } else {
                fields.insert(
                    name,
                    Shape {
                        kind,
                        repeated: false,
                    },
                );
            }
        } else if let Some(value) = child.text().filter(|_| child.is_text()) {
            text.push_str(value);
        }
    }
    if children {
        if !trim_xml_whitespace(&text).is_empty() {
            return Err(Error::UnsupportedXml(
                "cannot infer mixed text and child elements",
            ));
        }
    } else if fields.is_empty() {
        return Ok(scalar(&text));
    } else if !trim_xml_whitespace(&text).is_empty() {
        fields.insert(
            (None, ".".into()),
            Shape {
                kind: Kind::Scalar {
                    data_type: DataType::Utf8,
                    exact_float: true,
                },
                repeated: false,
            },
        );
    }
    Ok(Kind::Object(fields))
}

fn scalar(value: &str) -> Kind {
    let value = trim_xml_whitespace(value);
    if value.is_empty() {
        return Kind::Empty;
    }
    if let Ok(integer) = value.parse::<i64>() {
        return Kind::Scalar {
            data_type: DataType::Int64,
            exact_float: integer as f64 as i128 == i128::from(integer),
        };
    }
    let integer_literal = value
        .trim_start_matches(['+', '-'])
        .bytes()
        .all(|byte| byte.is_ascii_digit());
    Kind::Scalar {
        data_type: if matches!(value, "true" | "false") {
            DataType::Boolean
        } else if !integer_literal && value.parse::<f64>().is_ok_and(f64::is_finite) {
            DataType::Float64
        } else {
            DataType::Utf8
        },
        exact_float: true,
    }
}
