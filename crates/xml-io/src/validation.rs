use std::{borrow::Cow, io, path::Path};

use url::Url;

use crate::error::invalid;

pub(super) fn validate_object_path(path: &str) -> io::Result<()> {
    if path.is_empty() || path.ends_with('/') {
        return Err(invalid("input must name an object"));
    }
    Ok(())
}

pub(super) fn validate_local_path(uri: &str) -> io::Result<String> {
    if uri.is_empty() {
        return Err(invalid("input URI must not be empty"));
    }
    let path = if Path::new(uri).is_absolute() {
        uri.into()
    } else {
        match Url::parse(uri) {
            Ok(url) if url.scheme() == "file" => url
                .to_file_path()
                .map_err(|_| invalid("invalid file URL"))?,
            _ if uri.contains("://") => {
                return Err(invalid(
                    "use XmlFile::new with a configured operator for this storage service",
                ))
            }
            _ => uri.into(),
        }
    };
    let path = std::path::absolute(path)?;
    if path.file_name().is_none() {
        return Err(invalid("input must name a file"));
    }
    let path = path
        .into_os_string()
        .into_string()
        .map_err(|_| invalid("local path must be UTF-8"))?;
    Ok(path)
}

pub(super) fn local_path_parts(absolute: &str) -> io::Result<(&str, Cow<'_, str>)> {
    let absolute = Path::new(absolute);
    let root = absolute
        .ancestors()
        .last()
        .ok_or_else(|| invalid("missing filesystem root"))?;
    let path = absolute.strip_prefix(root).map_err(invalid)?;
    let path = path
        .to_str()
        .ok_or_else(|| invalid("local path must be UTF-8"))?;
    let root = root
        .to_str()
        .ok_or_else(|| invalid("local root must be UTF-8"))?;
    #[cfg(windows)]
    let path = Cow::Owned(path.replace('\\', "/"));
    #[cfg(not(windows))]
    let path = Cow::Borrowed(path);
    Ok((root, path))
}
