use std::{
    borrow::Cow,
    io::{self, Write},
};

use crate::{validation::valid_name, XmlBatchBuilder, XmlSchema};
use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use futures_util::io::{AsyncBufRead, AsyncReadExt, Chain, Cursor, Take};
use quick_xml::{
    events::{attributes::Attribute, BytesEnd, BytesStart, Event},
    name::{PrefixDeclaration, QName, ResolveResult},
    reader::NsReader,
    Writer, XmlVersion,
};
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt};

use super::{error::Result, Limit, ReadError, ReaderOptions};

#[derive(PartialEq, Eq)]
struct Name {
    uri: Option<String>,
    local: String,
}

struct Record {
    xml: String,
    byte_offset: u64,
}

struct Capture {
    writer: Writer<BoundedBuffer>,
    byte_offset: u64,
}

// Cursor replays the initial bytes when they are not a BOM; Chain prepends them
// to R. Take limits each event read; Compat adapts futures I/O for quick-xml.
type Input<R> = Compat<Take<Chain<Cursor<Vec<u8>>, R>>>;

/// Owns parsing and batching state for one XML document.
/// Selected elements are captured as complete records, then appended to builder.
pub(super) struct ReaderState<R> {
    // Parses events and tracks namespace bindings in the current element scope.
    reader: NsReader<Input<R>>,
    // Requested absolute path, resolved to namespace URI/local-name pairs.
    path: Vec<Name>,
    // Current element ancestry, including skipped elements; compared with path.
    stack: Vec<Name>,
    // Remains true after the root closes, so a second root can be rejected.
    root_seen: bool,
    // Stripped BOM length (0 or 3), added to parser offsets for source positions.
    bom_bytes: u64,
    // One configuration for path depth, record/event bytes, and batch limits.
    options: ReaderOptions,
    // Owns the projected schema, Arrow columns, and pending row count.
    builder: XmlBatchBuilder,
    // One record read ahead when adding it would exceed the batch byte limit.
    pending: Option<Record>,
    // One-based index of the last converted record; spans batch boundaries.
    record_index: u64,
}

impl<R: AsyncBufRead + Unpin> ReaderState<R> {
    pub(super) fn new(
        input: R,
        schema: XmlSchema,
        path: &str,
        options: ReaderOptions,
    ) -> Result<Self> {
        options.validate()?;
        let mut names = Vec::new();
        let path = path.strip_prefix('/').ok_or_else(|| {
            ReadError::Configuration("record path must be absolute, e.g. /catalog/book".into())
        })?;
        for part in path.split('/') {
            let (uri, local) = match part.split_once(':') {
                Some((prefix, local)) => (
                    Some(
                        schema
                            .namespaces
                            .get(prefix)
                            .ok_or_else(|| {
                                ReadError::Configuration(format!(
                                    "unbound record path prefix {prefix:?}"
                                ))
                            })?
                            .clone(),
                    ),
                    local,
                ),
                None => (None, part),
            };
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
        let mut reader =
            NsReader::from_reader(Cursor::new(Vec::new()).chain(input).take(0).compat());
        reader.config_mut().check_comments = true;
        Ok(Self {
            reader,
            path: names,
            stack: Vec::new(),
            root_seen: false,
            bom_bytes: 0,
            options,
            builder,
            pending: None,
            record_index: 0,
        })
    }

    pub(super) fn schema(&self) -> SchemaRef {
        self.builder.schema()
    }

    pub(super) async fn next_batch(&mut self) -> Result<Option<RecordBatch>> {
        // This scratch buffer is reused within the batch and retained by the
        // in-flight future if next_batch is cancelled.
        let mut buffer = Vec::new();
        let mut bytes = 0usize;
        let mut last_offset = 0;
        while !self.builder.is_full() {
            let record = match self.pending.take() {
                Some(record) => Some(record),
                None => self.next_record(&mut buffer).await?,
            };
            let Some(record) = record else {
                break;
            };
            if !self.builder.is_empty()
                && bytes.saturating_add(record.xml.len()) > self.options.max_batch_bytes
            {
                self.pending = Some(record);
                break;
            }
            self.record_index += 1;
            last_offset = record.byte_offset;
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
                byte_offset: last_offset,
                source,
            })
    }

    async fn next_record(&mut self, buffer: &mut Vec<u8>) -> Result<Option<Record>> {
        // A successful call consumes a complete record, so only the first call
        // can begin at parser position zero. No separate initialization flag.
        if self.reader.buffer_position() == 0 {
            // quick-xml's BOM removal requires three bytes in one fill_buf.
            // Read the prefix explicitly so single-byte buffers work too.
            let (prefix, input) = self.reader.get_mut().get_mut().get_mut().get_mut();
            let mut head = [0; 3];
            let mut len = 0;
            while len < head.len() {
                let n = input
                    .read(&mut head[len..])
                    .await
                    .map_err(|source| ReadError::xml(len as u64, source))?;
                if n == 0 {
                    break;
                }
                len += n;
            }
            self.bom_bytes = if head == [0xEF, 0xBB, 0xBF] {
                3
            } else {
                *prefix = Cursor::new(head[..len].to_vec());
                0
            };
        }
        // Capture is only needed until this call returns a complete record.
        let mut capture: Option<Capture> = None;
        loop {
            let before = self.position();
            // Take bounds parser allocations even for a huge unselected text,
            // comment, or tag. One extra byte distinguishes a full token from a
            // token which needs to peek at its following delimiter.
            let allowance = self.options.max_record_bytes as u64 + 1;
            self.reader.get_mut().get_mut().set_limit(allowance);
            let event = self.reader.read_event_into_async(buffer).await;
            if allowance - self.reader.get_mut().get_mut().limit()
                > self.options.max_record_bytes as u64
            {
                return Err(self.limit(Limit::EventBytes, self.options.max_record_bytes));
            }
            let event = event
                .map_err(|source| {
                    ReadError::xml(self.reader.buffer_position() + self.bom_bytes, source)
                })?
                .into_owned();
            buffer.clear();
            let empty = matches!(event, Event::Empty(_));
            match event {
                Event::Start(start) | Event::Empty(start) => {
                    self.enter(start.name())?;
                    if let Some(record) = &mut capture {
                        record.write(
                            if empty {
                                Event::Empty(start)
                            } else {
                                Event::Start(start)
                            },
                            self.position(),
                        )?;
                    } else {
                        let start = self.with_namespaces(start)?;
                        if self.stack == self.path {
                            let mut record = Capture {
                                writer: Writer::new(BoundedBuffer {
                                    bytes: Vec::new(),
                                    limit: self.options.max_record_bytes,
                                }),
                                byte_offset: before,
                            };
                            record.write(
                                if empty {
                                    Event::Empty(start)
                                } else {
                                    Event::Start(start)
                                },
                                self.position(),
                            )?;
                            capture = Some(record);
                        } else {
                            self.validate(Event::Empty(start), before)?;
                        }
                    }
                    if empty {
                        let complete = capture.is_some() && self.stack.len() == self.path.len();
                        self.stack.pop();
                        if complete {
                            return capture.unwrap().finish().map(Some);
                        }
                    }
                }
                Event::End(end) => {
                    if let Some(record) = &mut capture {
                        record.write(Event::End(end), self.position())?;
                        let complete = self.stack.len() == self.path.len();
                        self.stack.pop();
                        if complete {
                            return capture.unwrap().finish().map(Some);
                        }
                    } else {
                        self.stack.pop();
                    }
                }
                Event::DocType(_) => {
                    return Err(self.invalid(before, "DTDs and custom entities are not supported"))
                }
                Event::Eof => {
                    if !self.root_seen || !self.stack.is_empty() {
                        return Err(self.invalid(before, "expected one complete XML document"));
                    }
                    return Ok(None);
                }
                Event::Decl(decl) => {
                    if before != self.bom_bytes {
                        return Err(
                            self.invalid(before, "XML declaration must be at the beginning")
                        );
                    }
                    self.validate(Event::Decl(decl), before)?;
                }
                event => {
                    if let Some(record) = &mut capture {
                        record.write(event, self.position())?;
                    } else {
                        if self.stack.is_empty() {
                            match &event {
                                Event::Text(text)
                                    if text.as_ref().iter().all(|byte| {
                                        matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
                                    }) => {}
                                Event::Comment(_) | Event::PI(_) => {}
                                _ => {
                                    return Err(self
                                        .invalid(before, "content outside the document element"))
                                }
                            }
                        }
                        self.validate(event, before)?;
                    }
                }
            }
        }
    }

    fn enter(&mut self, name: QName<'_>) -> Result<()> {
        if self.stack.len() == self.options.max_depth {
            return Err(self.limit(Limit::Depth, self.options.max_depth));
        }
        if self.stack.is_empty() {
            if self.root_seen {
                return Err(self.invalid(self.position(), "multiple document elements"));
            }
            self.root_seen = true;
        }
        let (namespace, local) = self.reader.resolver().resolve_element(name);
        let uri = match namespace {
            ResolveResult::Unbound => None,
            ResolveResult::Bound(uri) => {
                let attr = Attribute {
                    key: QName(b"xmlns"),
                    value: Cow::Borrowed(uri.into_inner()),
                };
                Some(
                    attr.normalized_value(XmlVersion::Implicit1_0)
                        .map_err(|source| ReadError::xml(self.position(), source))?
                        .into_owned(),
                )
                .filter(|uri| !uri.is_empty())
            }
            ResolveResult::Unknown(prefix) => {
                return Err(self.invalid(
                    self.position(),
                    &format!(
                        "unknown namespace prefix {:?}",
                        String::from_utf8_lossy(&prefix)
                    ),
                ))
            }
        };
        let local = std::str::from_utf8(local.as_ref())
            .map_err(|source| ReadError::xml(self.position(), source))?
            .to_owned();
        self.stack.push(Name { uri, local });
        Ok(())
    }

    fn with_namespaces(&self, mut start: BytesStart<'static>) -> Result<BytesStart<'static>> {
        let keys = start
            .attributes()
            .map(|attr| attr.map(|attr| attr.key.as_ref().to_vec()))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|source| ReadError::xml(self.position(), source))?;
        for (prefix, uri) in self.reader.resolver().bindings() {
            let key = match prefix {
                PrefixDeclaration::Default => b"xmlns".to_vec(),
                PrefixDeclaration::Named(prefix) => [b"xmlns:".as_slice(), prefix].concat(),
            };
            if !keys.contains(&key) {
                // Bindings already contain XML escapes. Protect the output
                // quote without double-escaping references such as &amp;.
                let value = std::str::from_utf8(uri.into_inner())
                    .map_err(|source| ReadError::xml(self.position(), source))?
                    .replace('"', "&quot;");
                start.push_attribute((key.as_slice(), value.as_bytes()));
            }
        }
        Ok(start)
    }

    // Validate skipped tokens with the same XML library used by the converter;
    // quick-xml alone does not check every XML name, character, or attribute.
    fn validate(&self, event: Event<'_>, position: u64) -> Result<()> {
        let mut writer = Writer::new(Vec::new());
        match event {
            Event::Empty(_) => writer.write_event(event),
            Event::Decl(_) => writer
                .write_event(event)
                .and_then(|_| writer.write_event(Event::Empty(BytesStart::new("check")))),
            _ => writer
                .write_event(Event::Start(BytesStart::new("check")))
                .and_then(|_| writer.write_event(event))
                .and_then(|_| writer.write_event(Event::End(BytesEnd::new("check")))),
        }
        .map_err(|source| ReadError::xml(position, source))?;
        let xml = String::from_utf8(writer.into_inner())
            .map_err(|source| ReadError::xml(position, source))?;
        roxmltree::Document::parse(&xml).map_err(|source| ReadError::xml(position, source))?;
        Ok(())
    }

    fn position(&self) -> u64 {
        self.reader.buffer_position() + self.bom_bytes
    }

    fn invalid(&self, byte_offset: u64, message: &str) -> ReadError {
        ReadError::xml(
            byte_offset,
            io::Error::new(io::ErrorKind::InvalidData, message.to_owned()),
        )
    }

    fn limit(&self, kind: Limit, limit: usize) -> ReadError {
        ReadError::LimitExceeded {
            kind,
            limit,
            byte_offset: self.position(),
        }
    }
}

impl Capture {
    fn write(&mut self, event: Event<'_>, byte_offset: u64) -> Result<()> {
        let limit = self.writer.get_ref().limit;
        self.writer
            .write_event(event)
            .map_err(|_| ReadError::LimitExceeded {
                kind: Limit::RecordBytes,
                limit,
                byte_offset,
            })
    }

    fn finish(self) -> Result<Record> {
        let xml = String::from_utf8(self.writer.into_inner().bytes)
            .map_err(|source| ReadError::xml(self.byte_offset, source))?;
        Ok(Record {
            xml,
            byte_offset: self.byte_offset,
        })
    }
}

struct BoundedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for BoundedBuffer {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if input.len() > self.limit - self.bytes.len() {
            return Err(io::Error::other("XML record exceeds byte limit"));
        }
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
