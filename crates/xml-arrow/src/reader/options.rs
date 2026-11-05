const DEFAULT_BATCH_SIZE: usize = 1024;
const DEFAULT_MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_BATCH_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_MAX_DEPTH: usize = 128;

/// Batch sizing, projection, and input resource limits.
#[derive(Clone, Debug)]
pub struct ReaderOptions {
    /// Maximum rows returned per batch. Default: 1024.
    pub batch_size: usize,
    /// Maximum serialized bytes per record, including inherited namespaces.
    /// Also limits bytes consumed for each XML event. Default: 8 MiB.
    /// Record size is checked after each serialized event, so temporary buffers
    /// may exceed this limit.
    pub max_record_bytes: usize,
    /// Maximum sum of serialized input-record bytes represented by one batch.
    /// Arrow allocations depend on the schema; this is not an RSS limit.
    /// Must be at least `max_record_bytes`. Default: 64 MiB.
    pub max_batch_bytes: usize,
    /// Maximum element depth across the whole document. Default: 128.
    /// Values must be in `1..65535`.
    pub max_depth: usize,
    /// Root-column indices in output order. `None` selects all; `Some(vec![])`
    /// counts records without output columns.
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
