use crate::{validation::valid_name, XmlSchema};

use super::{error::Result, Name, ReadError, ReaderOptions};

pub(super) fn validate_reader(
    schema: &XmlSchema,
    path: &str,
    options: &ReaderOptions,
) -> Result<Vec<Name>> {
    if options.batch_size == 0
        || options.max_record_bytes == 0
        || options.max_record_bytes.checked_add(1).is_none()
        || options.max_batch_bytes < options.max_record_bytes
        || !(1..u16::MAX as usize).contains(&options.max_depth)
    {
        return Err(ReadError::Configuration("positive batch/record limits, max_batch_bytes >= max_record_bytes, and max_depth in 1..65535 are required".into()));
    }
    let mut names = Vec::new();
    let path = path.strip_prefix('/').ok_or_else(|| {
        ReadError::Configuration("record path must be absolute, e.g. /catalog/book".into())
    })?;
    for part in path.split('/') {
        let (prefix, local) = part
            .split_once(':')
            .map_or((None, part), |(prefix, local)| (Some(prefix), local));
        let uri = prefix
            .map(|prefix| {
                schema.namespaces.get(prefix).cloned().ok_or_else(|| {
                    ReadError::Configuration(format!("unbound record path prefix {prefix:?}"))
                })
            })
            .transpose()?;
        if !valid_name(local) {
            return Err(ReadError::Configuration(format!(
                "invalid record path segment {part:?}"
            )));
        }
        names.push(Name {
            uri,
            local: local.into(),
        });
    }
    if names.len() > options.max_depth {
        return Err(ReadError::Configuration(
            "record path exceeds max_depth".into(),
        ));
    }
    Ok(names)
}
