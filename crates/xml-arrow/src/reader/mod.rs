use crate::{XmlBatchBuilder, XmlSchema};
use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use futures_util::{
    io::AsyncBufRead,
    stream::{self, BoxStream},
    StreamExt, TryStreamExt,
};

mod error;
mod options;
mod records;

pub use error::{Limit, ReadError, Result as ReaderResult};
pub use options::ReaderOptions;

use error::Result;
use records::{Record, RecordReader};

/// Reads one XML document as batches of elements selected by an absolute path.
///
/// Inputs implement `futures::io::AsyncBufRead + Unpin + Send` and may borrow data
/// for `'a`. Pending parsing/batch work is owned by the internal stream, so
/// dropping a pending `next_batch()` future preserves it for a later call.
/// The first error discards the unfinished batch and ends the reader. Batches
/// already returned remain valid; unread document tails are validated as consumed.
pub struct XmlReader<'a> {
    batches: BoxStream<'a, Result<RecordBatch>>,
    schema: SchemaRef,
}

impl<'a> XmlReader<'a> {
    /// Creates a reader without reading input. Record paths include the document
    /// root, e.g. `/catalog/book`; prefixes use the supplied schema's URI bindings.
    pub fn new<R: AsyncBufRead + Unpin + Send + 'a>(
        input: R,
        schema: XmlSchema,
        record_path: &str,
        options: ReaderOptions,
    ) -> Result<Self> {
        options.validate()?;
        let records = RecordReader::new(input, &schema, record_path, &options)?;
        let builder = match options.projection.as_deref() {
            Some(indices) => XmlBatchBuilder::new_projected(schema, options.batch_size, indices),
            None => XmlBatchBuilder::new(schema, options.batch_size),
        }
        .map_err(|error| {
            ReadError::Configuration(match error {
                crate::Error::Configuration(message) => message,
                error => error.to_string(),
            })
        })?;
        let schema = builder.schema();
        let state = State {
            records,
            builder,
            max_batch_bytes: options.max_batch_bytes,
            pending: None,
            record_index: 0,
            last_offset: 0,
        };
        // try_unfold retains its in-flight future across Stream::poll_next calls.
        // Cancellation of next_batch must not drop quick-xml's partial event.
        let batches = stream::try_unfold(state, |mut state| async move {
            Ok(state.next_batch().await?.map(|batch| (batch, state)))
        })
        .boxed();
        Ok(Self { batches, schema })
    }

    /// Arrow output schema, including column projection.
    pub fn schema(&self) -> SchemaRef {
        self.schema.clone()
    }

    /// Returns the next batch, `None` at EOF, or the first error. Pending calls
    /// may be cancelled and retried. After an error, future calls return `None`.
    pub async fn next_batch(&mut self) -> Result<Option<RecordBatch>> {
        self.batches.try_next().await
    }

    /// Consumes the reader as a Send stream; stops after EOF or its first error.
    pub fn into_stream(self) -> BoxStream<'a, Result<RecordBatch>> {
        self.batches
    }
}

struct State<R> {
    records: RecordReader<R>,
    builder: XmlBatchBuilder,
    max_batch_bytes: usize,
    pending: Option<Record>,
    record_index: u64,
    last_offset: u64,
}

impl<R: AsyncBufRead + Unpin> State<R> {
    async fn next_batch(&mut self) -> Result<Option<RecordBatch>> {
        let mut bytes = 0usize;
        while !self.builder.is_full() {
            let record = match self.pending.take() {
                Some(record) => Some(record),
                None => self.records.next_record().await?,
            };
            let Some(record) = record else {
                break;
            };
            if !self.builder.is_empty()
                && bytes.saturating_add(record.xml.len()) > self.max_batch_bytes
            {
                self.pending = Some(record);
                break;
            }
            self.record_index += 1;
            self.last_offset = record.byte_offset;
            self.builder
                .append_xml(&record.xml)
                .map_err(|source| ReadError::Conversion {
                    record_index: self.record_index,
                    byte_offset: record.byte_offset,
                    source,
                })?;
            bytes += record.xml.len();
        }
        self.builder
            .flush()
            .map_err(|source| ReadError::Conversion {
                record_index: self.record_index,
                byte_offset: self.last_offset,
                source,
            })
    }
}
