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

/// Reads XML records selected by an absolute path into Arrow batches.
pub struct XmlReader<'a> {
    batches: BoxStream<'a, Result<RecordBatch>>,
    // Keep the schema available after EOF.
    schema: SchemaRef,
}

impl<'a> XmlReader<'a> {
    /// Creates a reader without reading input. Path prefixes use schema bindings.
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

    /// Projected output schema.
    pub fn schema(&self) -> SchemaRef {
        self.schema.clone()
    }

    /// Returns a batch or EOF. The first error discards pending rows and ends input.
    /// Cancelled calls can resume; calls after an error return `None`.
    pub async fn next_batch(&mut self) -> Result<Option<RecordBatch>> {
        self.batches.try_next().await
    }

    /// Consumes the reader as a batch stream.
    pub fn into_stream(self) -> BoxStream<'a, Result<RecordBatch>> {
        self.batches
    }
}
