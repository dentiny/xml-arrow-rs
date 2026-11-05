use std::{
    borrow::Cow,
    io::{self, Write},
};

use crate::{validation::valid_name, XmlSchema};
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

pub(super) struct Record {
    pub(super) xml: String,
    pub(super) byte_offset: u64,
}

struct Capture {
    writer: Writer<BoundedBuffer>,
    byte_offset: u64,
}

type Input<R> = Compat<Take<Chain<Cursor<Vec<u8>>, R>>>;

pub(super) struct RecordReader<R> {
    reader: NsReader<Input<R>>,
    buffer: Vec<u8>,
    path: Vec<Name>,
    stack: Vec<Name>,
    capture: Option<Capture>,
    root_seen: bool,
    initialized: bool,
    bom_bytes: u64,
    max_record_bytes: usize,
    max_depth: usize,
}

impl<R: AsyncBufRead + Unpin> RecordReader<R> {
    pub(super) fn new(
        input: R,
        schema: &XmlSchema,
        path: &str,
        options: &ReaderOptions,
    ) -> Result<Self> {
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
        let mut reader =
            NsReader::from_reader(Cursor::new(Vec::new()).chain(input).take(0).compat());
        reader.config_mut().check_comments = true;
        Ok(Self {
            reader,
            buffer: Vec::new(),
            path: names,
            stack: Vec::new(),
            capture: None,
            root_seen: false,
            initialized: false,
            bom_bytes: 0,
            max_record_bytes: options.max_record_bytes,
            max_depth: options.max_depth,
        })
    }

    pub(super) async fn next_record(&mut self) -> Result<Option<Record>> {
        if !self.initialized {
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
            if head == [0xEF, 0xBB, 0xBF] {
                self.bom_bytes = 3;
            } else {
                *prefix = Cursor::new(head[..len].to_vec());
            }
            self.initialized = true;
        }
        loop {
            let before = self.position();
            // Take bounds parser allocations even for a huge unselected text,
            // comment, or tag. One extra byte distinguishes a full token from a
            // token which needs to peek at its following delimiter.
            let allowance = self.max_record_bytes as u64 + 1;
            self.reader.get_mut().get_mut().set_limit(allowance);
            let event = self.reader.read_event_into_async(&mut self.buffer).await;
            if allowance - self.reader.get_mut().get_mut().limit() > self.max_record_bytes as u64 {
                return Err(self.limit(Limit::EventBytes, self.max_record_bytes));
            }
            let event = event
                .map_err(|source| {
                    ReadError::xml(self.reader.buffer_position() + self.bom_bytes, source)
                })?
                .into_owned();
            self.buffer.clear();
            let empty = matches!(event, Event::Empty(_));
            match event {
                Event::Start(start) | Event::Empty(start) => {
                    self.enter(start.name())?;
                    if self.capture.is_some() {
                        self.write(if empty {
                            Event::Empty(start)
                        } else {
                            Event::Start(start)
                        })?;
                    } else {
                        let start = self.with_namespaces(start)?;
                        if self.stack == self.path {
                            self.capture = Some(Capture {
                                writer: Writer::new(BoundedBuffer {
                                    bytes: Vec::new(),
                                    limit: self.max_record_bytes,
                                }),
                                byte_offset: before,
                            });
                            self.write(if empty {
                                Event::Empty(start)
                            } else {
                                Event::Start(start)
                            })?;
                        } else {
                            self.validate(Event::Empty(start), before)?;
                        }
                    }
                    if empty {
                        let complete =
                            self.capture.is_some() && self.stack.len() == self.path.len();
                        self.stack.pop();
                        if complete {
                            return self.finish().map(Some);
                        }
                    }
                }
                Event::End(end) => {
                    if self.capture.is_some() {
                        self.write(Event::End(end))?;
                        let complete = self.stack.len() == self.path.len();
                        self.stack.pop();
                        if complete {
                            return self.finish().map(Some);
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
                    if self.capture.is_some() {
                        self.write(event)?;
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
        if self.stack.len() == self.max_depth {
            return Err(self.limit(Limit::Depth, self.max_depth));
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

    fn write(&mut self, event: Event<'_>) -> Result<()> {
        let position = self.position();
        self.capture
            .as_mut()
            .unwrap()
            .writer
            .write_event(event)
            .map_err(|_| ReadError::LimitExceeded {
                kind: Limit::RecordBytes,
                limit: self.max_record_bytes,
                byte_offset: position,
            })
    }

    fn finish(&mut self) -> Result<Record> {
        let capture = self.capture.take().unwrap();
        let xml = String::from_utf8(capture.writer.into_inner().bytes)
            .map_err(|source| ReadError::xml(capture.byte_offset, source))?;
        Ok(Record {
            xml,
            byte_offset: capture.byte_offset,
        })
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
