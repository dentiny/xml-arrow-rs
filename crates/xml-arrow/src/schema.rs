use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use arrow_schema::{DataType, Field, Schema, SchemaRef};

use crate::{
    validation::{validate_fields, validate_namespaces},
    Error, Result,
};

/// An Arrow field and its XML path, relative to its parent element.
#[derive(Clone, Debug)]
pub struct XmlField {
    pub(crate) field: Field,
    pub(crate) path: String,
    pub(crate) children: Vec<XmlField>,
}

impl XmlField {
    /// Describes a scalar column. [`XmlSchema::new`] validates its name, path,
    /// and type. Use [`Self::structure`] or [`Self::list`] for nested mappings.
    ///
    /// `.` selects record text, `@id` a record attribute, `title` child text,
    /// and `author/@id` an attribute on a descendant element.
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

    /// Maps one element to a struct. Child paths are relative to that element.
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

    /// Maps one container element to a list. The item path selects repeated values
    /// inside the container. An existing container with no matching items is empty;
    /// a missing or `xsi:nil` container is null if nullable, otherwise an error.
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

    /// Arrow field including its name, type, and nullability.
    pub fn arrow_field(&self) -> &Field {
        &self.field
    }

    /// XML path relative to the parent element (the record for top-level fields).
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

/// Validated, ordered column mappings, namespace bindings, and Arrow schema.
#[derive(Clone, Debug)]
pub struct XmlSchema {
    pub(crate) fields: Vec<XmlField>,
    pub(crate) arrow: SchemaRef,
    pub(crate) namespaces: HashMap<String, String>,
}

impl XmlSchema {
    /// Validates column mappings. Empty schemas are supported and count rows.
    ///
    /// Supports scalar types, structs, and lists. Unprefixed paths select names
    /// without a namespace. `xml` is predefined; use [`Self::with_namespaces`]
    /// for other path prefixes. XPath expressions are not supported.
    pub fn new(fields: Vec<XmlField>) -> Result<Self> {
        Self::with_namespaces(fields, &[])
    }

    /// Binds schema path prefixes to namespace URIs. Prefixes in the input XML
    /// may differ, including use of a default namespace. Bindings also apply to
    /// attributes and nested fields. `xml` and `xmlns` cannot be rebound.
    pub fn with_namespaces(fields: Vec<XmlField>, bindings: &[(&str, &str)]) -> Result<Self> {
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

    /// The Arrow output schema.
    pub fn arrow_schema(&self) -> SchemaRef {
        Arc::clone(&self.arrow)
    }

    pub(crate) fn project(&self, projection: &[usize]) -> Result<Self> {
        let mut seen = HashSet::new();
        let mut fields = Vec::with_capacity(projection.len());
        for &index in projection {
            let field = self.fields.get(index).ok_or_else(|| {
                Error::Configuration(format!("projection index {index} is out of bounds"))
            })?;
            if !seen.insert(index) {
                return Err(Error::Configuration(format!(
                    "duplicate projection index {index}"
                )));
            }
            fields.push(field.clone());
        }
        let arrow = Arc::new(self.arrow.project(projection)?);
        Ok(Self {
            fields,
            arrow,
            namespaces: self.namespaces.clone(),
        })
    }
}
