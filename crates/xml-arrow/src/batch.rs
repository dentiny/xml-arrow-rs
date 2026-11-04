use std::sync::Arc;

use arrow_array::{
    builder::{BooleanBuilder, Float64Builder, Int64Builder, StringBuilder},
    ArrayRef, RecordBatch, RecordBatchOptions,
};
use arrow_schema::SchemaRef;

use crate::{
    parse::{parse, Scalar},
    Error, XmlSchema,
};

enum Column {
    Text(StringBuilder),
    Boolean(BooleanBuilder),
    Integer(Int64Builder),
    Float(Float64Builder),
}

impl Column {
    fn append(&mut self, value: Option<Scalar>) {
        match (self, value) {
            (Self::Text(b), Some(Scalar::Text(v))) => b.append_value(v),
            (Self::Boolean(b), Some(Scalar::Boolean(v))) => b.append_value(v),
            (Self::Integer(b), Some(Scalar::Integer(v))) => b.append_value(v),
            (Self::Float(b), Some(Scalar::Float(v))) => b.append_value(v),
            (Self::Text(b), None) => b.append_null(),
            (Self::Boolean(b), None) => b.append_null(),
            (Self::Integer(b), None) => b.append_null(),
            (Self::Float(b), None) => b.append_null(),
            _ => unreachable!("conversion uses the same validated schema as the builders"),
        }
    }

    fn finish(&mut self) -> ArrayRef {
        match self {
            Self::Text(b) => Arc::new(b.finish()),
            Self::Boolean(b) => Arc::new(b.finish()),
            Self::Integer(b) => Arc::new(b.finish()),
            Self::Float(b) => Arc::new(b.finish()),
        }
    }
}

/// Accumulates complete XML records into Arrow batches without performing I/O.
///
/// An append is atomic: all selected values are parsed and converted before any
/// column is changed. Flush a full batch before appending the next record.
/// The row limit does not bound record size or total bytes in memory.
pub struct XmlBatchBuilder {
    schema: XmlSchema,
    columns: Vec<Column>,
    batch_size: usize,
    rows: usize,
}

impl XmlBatchBuilder {
    /// Creates a builder for all columns. `batch_size` must be positive.
    pub fn new(schema: XmlSchema, batch_size: usize) -> Result<Self, Error> {
        if batch_size == 0 {
            return Err(Error::Configuration("batch_size must be positive".into()));
        }
        use arrow_schema::DataType;
        let columns = schema
            .fields
            .iter()
            .map(|f| match f.field.data_type() {
                DataType::Utf8 => Column::Text(StringBuilder::new()),
                DataType::Boolean => Column::Boolean(BooleanBuilder::new()),
                DataType::Int64 => Column::Integer(Int64Builder::new()),
                DataType::Float64 => Column::Float(Float64Builder::new()),
                _ => unreachable!("schema types are validated on construction"),
            })
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
    ) -> Result<Self, Error> {
        Self::new(schema.project(projection)?, batch_size)
    }

    /// Appends one complete XML record element, including optional surrounding
    /// whitespace, comments, processing instructions, and a leading XML declaration.
    ///
    /// Failure leaves previously appended rows intact and this builder reusable.
    pub fn append_xml(&mut self, input: &str) -> Result<(), Error> {
        if self.is_full() {
            return Err(Error::BatchFull);
        }
        let row = parse(input, &self.schema.fields)?;
        // Check every Utf8 offset before mutating any column. Arrow uses i32
        // offsets and its string builder would otherwise panic on overflow.
        for ((column, value), field) in self.columns.iter().zip(&row).zip(&self.schema.fields) {
            if let (Column::Text(builder), Some(Scalar::Text(value))) = (column, value) {
                if builder.values_slice().len().saturating_add(value.len()) > i32::MAX as usize {
                    return Err(
                        field.error("Utf8 offset limit exceeded; use a smaller batch or record")
                    );
                }
            }
        }
        for (column, value) in self.columns.iter_mut().zip(row) {
            column.append(value);
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
    pub fn flush(&mut self) -> Result<Option<RecordBatch>, Error> {
        if self.is_empty() {
            return Ok(None);
        }
        let columns = self.columns.iter_mut().map(Column::finish).collect();
        let options = RecordBatchOptions::new().with_row_count(Some(self.rows));
        let batch = RecordBatch::try_new_with_options(self.schema(), columns, &options)?;
        self.rows = 0;
        Ok(Some(batch))
    }
}
