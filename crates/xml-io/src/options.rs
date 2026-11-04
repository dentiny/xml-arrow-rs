use crate::{ReadError, Result};

/// Batch sizing, projection, and input resource limits.
#[derive(Clone, Debug)]
pub struct ReaderOptions {
    /// Maximum rows returned per batch. Default: 1024.
    pub batch_size: usize,
    /// Maximum serialized bytes per record, including inherited namespaces.
    /// Also limits bytes consumed for each XML event. Default: 8 MiB.
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
            batch_size: 1024,
            max_record_bytes: 8 * 1024 * 1024,
            max_batch_bytes: 64 * 1024 * 1024,
            max_depth: 128,
            projection: None,
        }
    }
}

impl ReaderOptions {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.batch_size == 0
            || self.max_record_bytes == 0
            || self.max_record_bytes.checked_add(1).is_none()
            || self.max_batch_bytes < self.max_record_bytes
            || !(1..u16::MAX as usize).contains(&self.max_depth)
        {
            return Err(ReadError::Configuration("positive batch/record limits, max_batch_bytes >= max_record_bytes, and max_depth in 1..65535 are required".into()));
        }
        Ok(())
    }
}
