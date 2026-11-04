use std::{collections::HashSet, sync::Arc};

use arrow_schema::{DataType, Field, Schema, SchemaRef};

use crate::Error;

/// One scalar Arrow column and its path relative to the XML record element.
#[derive(Clone, Debug)]
pub struct XmlField {
    pub(crate) field: Field,
    pub(crate) path: String,
}

impl XmlField {
    /// Describes a column. [`XmlSchema::new`] validates the name, path, and type.
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
        }
    }

    /// Arrow field including its name, type, and nullability.
    pub fn arrow_field(&self) -> &Field {
        &self.field
    }

    /// XML path relative to the record element.
    pub fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn error(&self, message: impl Into<String>) -> Error {
        Error::Field {
            name: self.field.name().clone(),
            path: self.path.clone(),
            message: message.into(),
        }
    }
}

/// Validated, ordered scalar column mappings and their Arrow schema.
#[derive(Clone, Debug)]
pub struct XmlSchema {
    pub(crate) fields: Vec<XmlField>,
    pub(crate) arrow: SchemaRef,
}

impl XmlSchema {
    /// Validates column mappings. Empty schemas are supported and count rows.
    ///
    /// Supported types are `Utf8`, `Boolean`, `Int64`, and `Float64`.
    /// Paths use unqualified XML names and `/`, with an optional terminal `@name`.
    /// Namespace matching and XPath expressions are not supported in this stage.
    pub fn new(fields: Vec<XmlField>) -> Result<Self, Error> {
        let mut names = HashSet::new();
        for field in &fields {
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
        let arrow = Arc::new(Schema::new(
            fields.iter().map(|f| f.field.clone()).collect::<Vec<_>>(),
        ));
        Ok(Self { fields, arrow })
    }

    /// Ordered column mappings.
    pub fn fields(&self) -> &[XmlField] {
        &self.fields
    }

    /// The Arrow output schema.
    pub fn arrow_schema(&self) -> SchemaRef {
        Arc::clone(&self.arrow)
    }

    pub(crate) fn project(&self, projection: &[usize]) -> Result<Self, Error> {
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
        Self::new(fields)
    }
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
