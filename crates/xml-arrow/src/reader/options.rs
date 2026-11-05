const DEFAULT_BATCH_SIZE: usize = 1024;
const DEFAULT_MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_BATCH_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_MAX_DEPTH: usize = 128;

/// Batch size, projection, and XML limits.
#[derive(Clone, Debug)]
pub struct ReaderOptions {
    /// Maximum rows returned per batch. Default: 1024.
    pub batch_size: usize,
    /// Maximum record bytes, including inherited namespaces. Default: 8 MiB.
    /// Also caps input events. Records are checked after writing each event;
    /// temporary buffers may exceed this limit.
    pub max_record_bytes: usize,
    /// Maximum sum of serialized record bytes per batch. Default: 64 MiB.
    /// Must be at least `max_record_bytes`; excludes Arrow allocations.
    pub max_batch_bytes: usize,
    /// Maximum element depth (`1..65535`). Default: 128.
    pub max_depth: usize,
    /// Output columns in order; `None` selects all, `Some(vec![])` only counts rows.
    pub projection: Option<Vec<usize>>,
}

impl Default for ReaderOptions {
    fn default() -> Self {
        Self {
            batch_size: DEFAULT_BATCH_SIZE,
            max_record_bytes: DEFAULT_MAX_RECORD_BYTES,
            max_batch_bytes: DEFAULT_MAX_BATCH_BYTES,
            max_depth: DEFAULT_MAX_DEPTH,
            projection: None,
        }
    }
}

const DEFAULT_MAX_SAMPLE_RECORDS: usize = 128;
const DEFAULT_MAX_SAMPLE_BYTES: usize = 8 * 1024 * 1024;

/// Bounds the records retained for schema inference.
#[derive(Clone, Debug)]
pub struct InferenceOptions {
    /// Maximum sampled records. Default: 128.
    pub max_records: usize,
    /// Maximum sampled serialized XML bytes. Default: 8 MiB.
    /// One additional record may be read ahead, bounded by `max_record_bytes`.
    pub max_bytes: usize,
}

impl Default for InferenceOptions {
    fn default() -> Self {
        Self {
            max_records: DEFAULT_MAX_SAMPLE_RECORDS,
            max_bytes: DEFAULT_MAX_SAMPLE_BYTES,
        }
    }
}
