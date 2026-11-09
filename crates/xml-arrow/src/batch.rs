use arrow_array::{
    builder::{make_builder, ArrayBuilder},
    RecordBatch, RecordBatchOptions,
};
use arrow_schema::SchemaRef;

use crate::{
    builders::{append, validate_capacity},
    parse::parse,
    Error, Result, XmlSchema,
};

/// Builds Arrow batches from complete XML record elements.
pub struct XmlBatchBuilder {
    schema: XmlSchema,
    columns: Vec<Box<dyn ArrayBuilder>>,
    batch_size: usize,
    rows: usize,
}

impl XmlBatchBuilder {
    /// Creates a builder with a positive row limit.
    pub fn new(schema: XmlSchema, batch_size: usize) -> Result<Self> {
        if batch_size == 0 {
            return Err(Error::Configuration("batch_size must be positive".into()));
        }
        let columns = schema
            .fields
            .iter()
            .map(|f| make_builder(f.field.data_type(), batch_size.min(1024)))
            .collect();
        Ok(Self {
            schema,
            columns,
            batch_size,
            rows: 0,
        })
    }

    /// Selects columns in order; duplicate or invalid indices fail.
    /// Only selected fields are converted; an empty projection counts rows.
    /// XML syntax is always checked.
    pub fn new_projected(
        schema: XmlSchema,
        batch_size: usize,
        projection: &[usize],
    ) -> Result<Self> {
        Self::new(schema.project(projection)?, batch_size)
    }

    /// Appends a complete XML record. Errors leave pending rows unchanged.
    pub fn append_xml(&mut self, input: &str) -> Result<()> {
        if self.is_full() {
            return Err(Error::BatchFull);
        }
        let document = roxmltree::Document::parse(input)?;
        let row = parse(&document, &self.schema)?;
        for ((column, value), field) in self.columns.iter().zip(&row).zip(&self.schema.fields) {
            validate_capacity(column.as_ref(), field, &[value])?;
        }
        for ((column, value), field) in self.columns.iter_mut().zip(row).zip(&self.schema.fields) {
            append(column.as_mut(), field, value);
        }
        self.rows += 1;
        Ok(())
    }

    /// Pending row count.
    pub fn len(&self) -> usize {
        self.rows
    }

    /// Whether the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.rows == 0
    }

    /// Whether the row limit has been reached.
    pub fn is_full(&self) -> bool {
        self.rows == self.batch_size
    }

    /// Projected output schema.
    pub fn schema(&self) -> SchemaRef {
        self.schema.arrow_schema()
    }

    /// Flushes pending rows; returns `None` if empty.
    pub fn flush(&mut self) -> Result<Option<RecordBatch>> {
        if self.is_empty() {
            return Ok(None);
        }
        let columns = self
            .columns
            .iter_mut()
            .map(|column| column.finish())
            .collect();
        let options = RecordBatchOptions::new().with_row_count(Some(self.rows));
        let batch = RecordBatch::try_new_with_options(self.schema(), columns, &options)?;
        self.rows = 0;
        Ok(Some(batch))
    }
}
