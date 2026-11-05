use crate::XmlSchema;
use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use futures_util::{
    io::AsyncBufRead,
    stream::{self, BoxStream},
    StreamExt, TryStreamExt,
};

mod error;
mod options;
mod state;
mod validation;

pub use error::{Limit, ReadError, Result as ReaderResult};
pub use options::ReaderOptions;

use error::Result;
use state::ReaderState;

#[derive(PartialEq, Eq)]
struct Name {
    uri: Option<String>,
    local: String,
}

/// Reads one XML document as batches of elements selected by an absolute path.
///
/// Inputs implement `futures::io::AsyncBufRead + Unpin + Send` and may borrow data
/// for `'a`. Pending parsing/batch work is owned by the internal stream, so
/// dropping a pending `next_batch()` future preserves it for a later call.
/// The first error discards the unfinished batch and ends the reader. Batches
/// already returned remain valid; unread document tails are validated as consumed.
pub struct XmlReader<'a> {
    batches: BoxStream<'a, Result<RecordBatch>>,
    // Shared schema remains available after the stream drops its parsing state.
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
        let state = ReaderState::new(input, schema, record_path, options)?;
        let schema = state.schema();
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
