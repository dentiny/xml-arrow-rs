use crate::XmlSchema;
use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use futures_util::{
    io::AsyncBufRead,
    stream::{self, BoxStream},
    StreamExt, TryStreamExt,
};

mod error;
mod inference;
mod options;
mod state;
mod validation;

pub use error::{Limit, ReadError, Result as ReaderResult};
pub use options::{InferenceOptions, ReaderOptions};

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
        Ok(Self::from_state(ReaderState::new(
            input,
            schema,
            record_path,
            options,
        )?))
    }

    /// Infers a fixed schema from a bounded sample, then replays sampled records.
    /// Bindings resolve record-path prefixes; other namespaces get generated prefixes.
    pub async fn infer<R: AsyncBufRead + Unpin + Send + 'a>(
        input: R,
        record_path: &str,
        bindings: &[(&str, &str)],
        options: ReaderOptions,
        sampling: InferenceOptions,
    ) -> Result<Self> {
        Ok(Self::from_state(
            ReaderState::infer(input, record_path, bindings, options, sampling).await?,
        ))
    }

    fn from_state<R: AsyncBufRead + Unpin + Send + 'a>(state: ReaderState<R>) -> Self {
        let schema = state.schema();
        let batches = stream::try_unfold(state, |mut state| async move {
            Ok(state.next_batch().await?.map(|batch| (batch, state)))
        })
        .boxed();
        Self { batches, schema }
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
