use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use arrow_schema::{DataType, Field, Schema, SchemaRef};

use crate::{
    validation::{validate_fields, validate_namespaces},
    Error, Result,
};

/// An Arrow field with a relative XML path.
#[derive(Clone, Debug)]
pub struct XmlField {
    pub(crate) field: Field,
    pub(crate) path: String,
    pub(crate) children: Vec<XmlField>,
}

impl XmlField {
    /// Maps a scalar column to a relative path, such as `.`, `@id`, or `author/name`.
    pub fn new(
        name: impl Into<String>,
        path: impl Into<String>,
        data_type: DataType,
        nullable: bool,
    ) -> Self {
        Self {
            field: Field::new(name, data_type, nullable),
            path: path.into(),
            children: Vec::new(),
        }
    }

    /// Maps a struct with child paths relative to its element.
    pub fn structure(
        name: impl Into<String>,
        path: impl Into<String>,
        nullable: bool,
        children: Vec<XmlField>,
    ) -> Self {
        let data_type =
            DataType::Struct(children.iter().map(|child| child.field.clone()).collect());
        Self {
            field: Field::new(name, data_type, nullable),
            path: path.into(),
            children,
        }
    }

    /// Maps a list container; the item path selects repeated values inside it.
    /// Empty containers become `[]`; missing or nil nullable containers become null.
    pub fn list(
        name: impl Into<String>,
        path: impl Into<String>,
        nullable: bool,
        item: XmlField,
    ) -> Self {
        Self {
            field: Field::new(name, DataType::List(Arc::new(item.field.clone())), nullable),
            path: path.into(),
            children: vec![item],
        }
    }

    /// Arrow field definition.
    pub fn arrow_field(&self) -> &Field {
        &self.field
    }

    /// XML path relative to its parent element.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Struct children, one list item mapping, or an empty slice for a scalar.
    pub fn children(&self) -> &[XmlField] {
        &self.children
    }

    pub(crate) fn error(&self, message: impl Into<String>) -> Error {
        Error::Field {
            name: self.field.name().clone(),
            path: self.path.clone(),
            message: message.into(),
        }
    }
}

/// A schema prefix bound to a namespace URI.
#[derive(Clone, Copy, Debug)]
pub struct NamespaceBinding<'a> {
    /// Prefix used in schema and record paths.
    pub prefix: &'a str,
    /// XML namespace URI matched independently of input prefixes.
    pub uri: &'a str,
}

/// Validated column mappings and namespace bindings.
#[derive(Clone, Debug)]
pub struct XmlSchema {
    pub(crate) fields: Vec<XmlField>,
    arrow: SchemaRef,
    pub(crate) namespaces: HashMap<String, String>,
}

impl XmlSchema {
    /// Validates column mappings. An empty schema counts rows.
    pub fn new(fields: Vec<XmlField>) -> Result<Self> {
        Self::with_namespaces(fields, &[])
    }

    /// Binds path prefixes to namespace URIs; input prefixes may differ.
    /// `xml` is predefined; `xml` and `xmlns` cannot be rebound.
    pub fn with_namespaces(
        fields: Vec<XmlField>,
        bindings: &[NamespaceBinding<'_>],
    ) -> Result<Self> {
        let namespaces = validate_namespaces(bindings)?;
        validate_fields(&fields, &namespaces)?;
        let arrow = Arc::new(Schema::new(
            fields.iter().map(|f| f.field.clone()).collect::<Vec<_>>(),
        ));
        Ok(Self {
            fields,
            arrow,
            namespaces,
        })
    }

    /// Ordered column mappings.
    pub fn fields(&self) -> &[XmlField] {
        &self.fields
    }

    /// Arrow output schema.
    pub fn arrow_schema(&self) -> SchemaRef {
        Arc::clone(&self.arrow)
    }

    pub(crate) fn validate_projection(&self, projection: &[usize]) -> Result<()> {
        let mut seen = HashSet::new();
        for &index in projection {
            if index >= self.fields.len() {
                return Err(Error::Configuration(format!(
                    "projection index {index} is out of bounds"
                )));
            }
            if !seen.insert(index) {
                return Err(Error::Configuration(format!(
                    "duplicate projection index {index}"
                )));
            }
        }
        Ok(())
    }

    pub(crate) fn project(&self, projection: &[usize]) -> Result<Self> {
        self.validate_projection(projection)?;
        let fields = projection
            .iter()
            .map(|&index| self.fields[index].clone())
            .collect();
        let arrow = Arc::new(self.arrow.project(projection)?);
        Ok(Self {
            fields,
            arrow,
            namespaces: self.namespaces.clone(),
        })
    }
}
