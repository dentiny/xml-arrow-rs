use crate::{NamespaceBinding, XmlSchema};
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

/// One selected XML record, ready for independent Arrow conversion.
#[derive(Debug)]
pub struct XmlRecord {
    /// Complete record element with inherited namespace declarations attached.
    pub xml: String,
    /// Start of the record in the original document, in bytes.
    pub byte_offset: u64,
}

/// Stream complete XML records without converting field values to Arrow.
///
/// Uses the same path matching, namespaces, document validation and input limits
/// as [`XmlReader`]. Construction does not read input. The first error ends the
/// stream. Batch size and projection are validated but do not change record XML;
/// consumers can use [`crate::XmlBatchBuilder::new_projected`] for conversion.
pub fn read_records<'a, R: AsyncBufRead + Unpin + Send + 'a>(
    input: R,
    schema: XmlSchema,
    record_path: &str,
    options: ReaderOptions,
) -> Result<BoxStream<'a, Result<XmlRecord>>> {
    options.validate(&schema, record_path)?;
    // Keep namespace bindings without allocating unused Arrow column builders.
    let schema = schema.project(&[]).map_err(ReadError::configuration)?;
    let options = ReaderOptions {
        projection: None,
        ..options
    };
    let state = ReaderState::new(input, schema, record_path, options)?;
    Ok(record_stream(state))
}

/// Infer reusable XML mappings from a bounded sample, then replay every raw record.
///
/// The returned schema includes namespace bindings and all sampled fields, even
/// when `options.projection` selects fewer columns. Sampled records and any byte-
/// limit lookahead precede the unread input, preserving original byte offsets.
/// Limits, sampling and document validation match [`XmlReader::infer`]; conversion
/// is left to independent [`crate::XmlBatchBuilder`] instances.
pub async fn infer_records<'a, R: AsyncBufRead + Unpin + Send + 'a>(
    input: R,
    record_path: &str,
    bindings: &[NamespaceBinding<'_>],
    options: ReaderOptions,
    sampling: InferenceOptions,
) -> Result<(XmlSchema, BoxStream<'a, Result<XmlRecord>>)> {
    let (state, schema) =
        ReaderState::infer(input, record_path, bindings, options, sampling).await?;
    Ok((schema, record_stream(state)))
}

fn record_stream<'a, R: AsyncBufRead + Unpin + Send + 'a>(
    state: ReaderState<R>,
) -> BoxStream<'a, Result<XmlRecord>> {
    stream::try_unfold((state, Vec::new()), |(mut state, mut buffer)| async move {
        Ok(state
            .next_replay_record(&mut buffer)
            .await?
            .map(|record| (record, (state, buffer))))
    })
    .boxed()
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
        bindings: &[NamespaceBinding<'_>],
        options: ReaderOptions,
        sampling: InferenceOptions,
    ) -> Result<Self> {
        let (state, schema) =
            ReaderState::infer(input, record_path, bindings, options, sampling).await?;
        Ok(Self::from_state(state.with_schema(schema)?))
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
