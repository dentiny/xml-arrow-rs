use std::io::{self, Write};

use crate::{XmlBatchBuilder, XmlSchema};
use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use futures_util::io::{AsyncBufRead, AsyncReadExt, Chain, Cursor, Take};
use quick_xml::{
    events::{attributes::Attribute, BytesEnd, BytesStart, Event},
    name::{Namespace, PrefixDeclaration, QName},
    reader::NsReader,
    Writer, XmlVersion,
};
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt};

use super::{error::Result, validation::validate_reader, Limit, Name, ReadError, ReaderOptions};

struct Record {
    xml: String,
    byte_offset: u64,
}

struct Capture {
    writer: Writer<BoundedBuffer>,
    byte_offset: u64,
}

// Cursor exposes the whole three-byte prefix so quick-xml can handle a split BOM.
// Take bounds each event; Compat adapts futures I/O for quick-xml.
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
        let path = validate_reader(&schema, path, &options)?;
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
            path,
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
        if self.reader.buffer_position() == 0 {
            // Expose all three BOM bytes together, even for one-byte inputs.
            let (prefix, input) = self.reader.get_mut().get_mut().get_mut().get_mut();
            input
                .take(3)
                .read_to_end(prefix.get_mut())
                .await
                .map_err(|source| ReadError::xml(prefix.get_ref().len() as u64, source))?;
            self.bom_bytes = if prefix.get_ref() == b"\xEF\xBB\xBF" {
                3
            } else {
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
            // quick-xml consumes the BOM separately from its first XML event.
            let bom = if self.reader.buffer_position() == 0 {
                self.bom_bytes
            } else {
                0
            };
            let limit = (self.options.max_record_bytes as u64).saturating_add(bom);
            let allowance = limit.saturating_add(1);
            self.reader.get_mut().get_mut().set_limit(allowance);
            let event = self.reader.read_event_into_async(buffer).await;
            if allowance - self.reader.get_mut().get_mut().limit() > limit {
                return Err(self.limit(Limit::EventBytes, self.options.max_record_bytes));
            }
            let event = event
                .map_err(|source| {
                    ReadError::xml(self.reader.buffer_position() + self.bom_bytes, source)
                })?
                .into_owned();
            buffer.clear();
            let empty = matches!(event, Event::Empty(_));
            let closes = empty || matches!(event, Event::End(_));
            let event = match event {
                Event::Start(start) | Event::Empty(start) => {
                    self.enter(start.name())?;
                    let start = if capture.is_none() {
                        self.with_namespaces(start)?
                    } else {
                        start
                    };
                    if capture.is_none() && self.stack == self.path {
                        capture = Some(Capture {
                            writer: Writer::new(BoundedBuffer {
                                bytes: Vec::new(),
                                limit: self.options.max_record_bytes,
                            }),
                            byte_offset: before,
                        });
                    }
                    if empty {
                        Event::Empty(start)
                    } else {
                        Event::Start(start)
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
                Event::Decl(_) if before != self.bom_bytes => {
                    return Err(self.invalid(before, "XML declaration must be at the beginning"));
                }
                event => event,
            };
            if let Some(record) = &mut capture {
                record.write(event, self.position())?;
            } else if !matches!(event, Event::End(_)) {
                self.validate(event, before)?;
            }
            let complete = closes && capture.is_some() && self.stack.len() == self.path.len();
            if closes {
                self.stack.pop();
            }
            if complete {
                return capture.unwrap().finish().map(Some);
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
        let uri = Option::<Namespace<'_>>::try_from(namespace)
            .map_err(|source| ReadError::xml(self.position(), source))?
            .map(|uri| {
                Attribute::from((b"xmlns".as_slice(), uri.into_inner()))
                    .normalized_value(XmlVersion::Implicit1_0)
                    .map(|value| value.into_owned())
                    .map_err(|source| ReadError::xml(self.position(), source))
            })
            .transpose()?
            .filter(|uri| !uri.is_empty());
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
        if self.stack.is_empty() {
            let outside = match &event {
                Event::Text(text) => text
                    .as_ref()
                    .iter()
                    .all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n')),
                Event::Decl(_) | Event::Comment(_) | Event::PI(_) => true,
                _ => false,
            };
            if !outside {
                return Err(self.invalid(position, "content outside the document element"));
            }
        }
        let mut writer = Writer::new(Vec::new());
        match event {
            Event::Start(start) | Event::Empty(start) => writer.write_event(Event::Empty(start)),
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
