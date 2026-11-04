use arrow_array::{
    builder::{make_builder, ArrayBuilder},
    RecordBatch, RecordBatchOptions,
};
use arrow_schema::SchemaRef;

use crate::{
    builders::append, parse::parse, validation::validate_capacity, Error, Result, XmlSchema,
};

/// Accumulates complete XML records into Arrow batches without performing I/O.
///
/// An append is atomic: all selected values are parsed and converted before any
/// column is changed. Flush a full batch before appending the next record.
/// The row limit does not bound record size or total bytes in memory.
pub struct XmlBatchBuilder {
    schema: XmlSchema,
    columns: Vec<Box<dyn ArrayBuilder>>,
    batch_size: usize,
    rows: usize,
}

impl XmlBatchBuilder {
    /// Creates a builder for all columns. `batch_size` must be positive.
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

    /// Selects column indices in output order. Duplicates and out-of-range indices fail.
    ///
    /// An empty projection preserves row counts. Only selected fields are converted
    /// or checked for missing/repeated values; XML syntax and supported input rules
    /// are checked regardless of projection.
    pub fn new_projected(
        schema: XmlSchema,
        batch_size: usize,
        projection: &[usize],
    ) -> Result<Self> {
        Self::new(schema.project(projection)?, batch_size)
    }

    /// Appends one complete XML record element, including optional surrounding
    /// whitespace, comments, processing instructions, and a leading XML declaration.
    ///
    /// Failure leaves previously appended rows intact and this builder reusable.
    pub fn append_xml(&mut self, input: &str) -> Result<()> {
        if self.is_full() {
            return Err(Error::BatchFull);
        }
        let row = parse(input, &self.schema)?;
        // Check all nested offsets before mutating any column.
        for ((column, value), field) in self.columns.iter().zip(&row).zip(&self.schema.fields) {
            validate_capacity(column.as_ref(), field, &[value.as_ref()])?;
        }
        for ((column, value), field) in self.columns.iter_mut().zip(row).zip(&self.schema.fields) {
            append(column.as_mut(), field, value);
        }
        self.rows += 1;
        Ok(())
    }

    /// Number of pending rows.
    pub fn len(&self) -> usize {
        self.rows
    }

    /// Whether there are no pending rows.
    pub fn is_empty(&self) -> bool {
        self.rows == 0
    }

    /// Whether the configured row capacity has been reached.
    pub fn is_full(&self) -> bool {
        self.rows == self.batch_size
    }

    /// Output schema, including projection order.
    pub fn schema(&self) -> SchemaRef {
        self.schema.arrow_schema()
    }

    /// Returns pending rows and resets the builder. Returns `None` when empty.
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
