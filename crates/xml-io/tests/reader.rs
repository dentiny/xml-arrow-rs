use std::{
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

use arrow_array::{cast::AsArray, types::Int64Type};
use arrow_schema::DataType::{Int64, Utf8};
use futures_executor::block_on;
use futures_util::{
    io::{AsyncBufRead, AsyncRead, BufReader, Cursor},
    FutureExt, TryStreamExt,
};
use xml_arrow::{XmlField, XmlSchema};
use xml_io::{Limit, ReadError, ReaderOptions, XmlReader};

fn schema() -> XmlSchema {
    XmlSchema::new(vec![XmlField::new("id", "@id", Int64, false)]).unwrap()
}

// Small buffers plus a Pending before every fill exercise cancellation *inside*
// XML tokens. Counters observe source consumption and drop through the public API.
struct Input<'a> {
    bytes: &'a [u8],
    pos: usize,
    chunk: usize,
    pause: bool,
    stutter: bool,
    fail_at: Option<usize>,
    consumed: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
}

impl<'a> Input<'a> {
    fn new(bytes: &'a [u8], chunk: usize, stutter: bool) -> Self {
        Self {
            bytes,
            pos: 0,
            chunk,
            pause: stutter,
            stutter,
            fail_at: None,
            consumed: Arc::new(AtomicUsize::new(0)),
            dropped: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl AsyncBufRead for Input<'_> {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        let this = self.get_mut();
        if this.pause {
            this.pause = false;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        this.pause = this.stutter;
        if this.fail_at == Some(this.pos) {
            return Poll::Ready(Err(io::Error::other("injected input failure")));
        }
        let end = (this.pos + this.chunk)
            .min(this.bytes.len())
            .min(this.fail_at.unwrap_or(usize::MAX));
        Poll::Ready(Ok(&this.bytes[this.pos..end]))
    }
    fn consume(self: Pin<&mut Self>, amount: usize) {
        let this = self.get_mut();
        this.pos += amount;
        this.consumed.store(this.pos, Ordering::SeqCst);
    }
}

impl AsyncRead for Input<'_> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let bytes = match self.as_mut().poll_fill_buf(cx) {
            Poll::Ready(result) => result?,
            Poll::Pending => return Poll::Pending,
        };
        let len = bytes.len().min(out.len());
        out[..len].copy_from_slice(&bytes[..len]);
        self.consume(len);
        Poll::Ready(Ok(len))
    }
}

impl Drop for Input<'_> {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

#[test]
fn split_tokens_utf8_and_namespace_context_survive_batch_boundaries() {
    block_on(async {
        let schema = XmlSchema::with_namespaces(
            vec![
                XmlField::new("id", "@id", Int64, false),
                XmlField::new("name", "r:title", Utf8, true),
            ],
            &[("r", "urn:rows&records")],
        )
        .unwrap();
        let xml = r#"<root xmlns="urn:rows&amp;records" xmlns:n="http://www.w3.org/2001/XMLSchema-instance">
            <row id="1" label="a>b"><title>北<![CDATA[<京>]]>&amp;</title><!-- </row> --></row>
            <q:row xmlns:q="urn:rows&amp;records" id="2"><q:title>東京</q:title></q:row>
            <row id="3"><title n:nil="true"/></row>
            <row xmlns="" id="999"/>
        </root>"#;
        let input = BufReader::with_capacity(1, Cursor::new(xml.as_bytes()));
        let options = ReaderOptions {
            batch_size: 2,
            projection: Some(vec![1, 0]),
            ..Default::default()
        };
        let mut reader = XmlReader::new(input, schema, "/r:root/r:row", options).unwrap();
        assert_eq!(reader.schema().field(0).name(), "name");
        let first = reader.next_batch().await.unwrap().unwrap();
        assert_eq!(
            first
                .column(0)
                .as_string::<i32>()
                .iter()
                .collect::<Vec<_>>(),
            vec![Some("北<京>&"), Some("東京")]
        );
        assert_eq!(
            first
                .column(1)
                .as_primitive::<Int64Type>()
                .values()
                .as_ref(),
            &[1, 2]
        );
        let rest: Vec<_> = reader.into_stream().try_collect().await.unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].num_rows(), 1);
        assert_eq!(rest[0].column(0).as_string::<i32>().value(0), "");
        assert!(arrow_array::Array::is_null(rest[0].column(0).as_ref(), 0));
        assert_eq!(rest[0].column(1).as_primitive::<Int64Type>().value(0), 3);
    });
}

#[test]
fn cancelled_next_batch_resumes_without_losing_partial_events_or_rows() {
    let xml = String::from("<root><row id='1'/><row id='2' note='北😀'/></root>");
    let input = Input::new(xml.as_bytes(), 1, true);
    let mut reader =
        XmlReader::new(input, schema(), "/root/row", ReaderOptions::default()).unwrap();
    let mut cancelled = 0;
    let mut rows = Vec::new();
    let mut eof = false;
    for _ in 0..10000 {
        match reader.next_batch().now_or_never() {
            None => cancelled += 1,
            Some(Ok(Some(batch))) => rows.extend(
                batch
                    .column(0)
                    .as_primitive::<Int64Type>()
                    .values()
                    .iter()
                    .copied(),
            ),
            Some(Ok(None)) => {
                eof = true;
                break;
            }
            Some(Err(error)) => panic!("{error}"),
        }
    }
    assert!(cancelled > 20);
    assert!(eof);
    assert_eq!(rows, vec![1, 2]);
}

#[test]
fn first_conversion_or_io_error_discards_partial_batch_and_ends_input() {
    block_on(async {
        let xml = "<root>\n  <row id='1'/>\n  <row id='bad'/>\n  <row id='3'/>\n</root>";
        let mut reader = XmlReader::new(
            Cursor::new(xml.as_bytes()),
            schema(),
            "/root/row",
            ReaderOptions::default(),
        )
        .unwrap();
        let error = reader.next_batch().await.unwrap_err();
        assert!(
            matches!(error, ReadError::Conversion { record_index: 2, byte_offset, .. } if byte_offset == xml.find("<row id='bad'").unwrap() as u64)
        );
        assert!(reader.next_batch().await.unwrap().is_none());
        assert!(reader.next_batch().await.unwrap().is_none());
        let mut reader = XmlReader::new(
            Input::new(b"\xEF\xBB\xBF<row id='bad'/>", 1, true),
            schema(),
            "/row",
            ReaderOptions::default(),
        )
        .unwrap();
        assert!(matches!(
            reader.next_batch().await,
            Err(ReadError::Conversion {
                record_index: 1,
                byte_offset: 3,
                ..
            })
        ));
        let mut input = Input::new(xml.as_bytes(), 3, true);
        input.fail_at = Some(xml.find("<row id='bad'").unwrap());
        let mut reader =
            XmlReader::new(input, schema(), "/root/row", ReaderOptions::default()).unwrap();
        let error = reader.next_batch().await.unwrap_err();
        assert!(error.to_string().contains("injected input failure"));
        assert!(std::error::Error::source(&error).is_some());
        assert!(reader.next_batch().await.unwrap().is_none());
    });
}

#[test]
fn limits_apply_to_unselected_content_and_batches_keep_lookahead_records() {
    block_on(async {
        let cases = [
            (
                "<root><skip><deep/></skip></root>".to_owned(),
                ReaderOptions {
                    max_depth: 2,
                    ..Default::default()
                },
                Limit::Depth,
            ),
            (
                format!("<root><!--{}--></root>", "x".repeat(5000)),
                ReaderOptions {
                    max_record_bytes: 32,
                    max_batch_bytes: 64,
                    ..Default::default()
                },
                Limit::EventBytes,
            ),
            (
                "<root><row><v>123456789012345</v><v>123456789012345</v></row></root>".to_owned(),
                ReaderOptions {
                    max_record_bytes: 40,
                    max_batch_bytes: 80,
                    ..Default::default()
                },
                Limit::RecordBytes,
            ),
        ];
        for (xml, options, expected) in cases {
            let input = Input::new(xml.as_bytes(), 8, false);
            let consumed = input.consumed.clone();
            let mut reader =
                XmlReader::new(input, XmlSchema::new(vec![]).unwrap(), "/root/row", options)
                    .unwrap();
            assert!(
                matches!(reader.next_batch().await, Err(ReadError::LimitExceeded { kind, .. }) if kind == expected)
            );
            if expected == Limit::EventBytes {
                assert!(consumed.load(Ordering::SeqCst) < 100);
            }
            assert!(reader.next_batch().await.unwrap().is_none());
        }
        let options = ReaderOptions {
            batch_size: 10,
            max_record_bytes: 8,
            max_batch_bytes: 13,
            projection: Some(vec![]),
            ..Default::default()
        };
        let mut reader = XmlReader::new(
            Cursor::new(b"<root><row/><row/><row/></root>"),
            schema(),
            "/root/row",
            options,
        )
        .unwrap();
        let first = reader.next_batch().await.unwrap().unwrap();
        assert_eq!((first.num_rows(), first.num_columns()), (2, 0));
        assert_eq!(reader.next_batch().await.unwrap().unwrap().num_rows(), 1);
        assert!(reader.next_batch().await.unwrap().is_none());
    });
}

#[test]
fn malformed_documents_fail_and_configuration_is_checked_before_io() {
    block_on(async {
        for xml in [
            "",
            "<root><row/></root",
            "<root/><root/>",
            "<!DOCTYPE root><root/>",
            "<root><skip a='1' a='2'/></root>",
            "<root><1bad/></root>",
            "<root><skip>&unknown;</skip></root>",
        ] {
            let mut reader = XmlReader::new(
                Cursor::new(xml.as_bytes()),
                schema(),
                "/root/row",
                ReaderOptions::default(),
            )
            .unwrap();
            assert!(reader.next_batch().await.is_err(), "{xml}");
            assert!(reader.next_batch().await.unwrap().is_none());
        }
        let mut empty = XmlReader::new(
            Cursor::new("\u{FEFF}<?xml version='1.0'?><root/>".as_bytes()),
            schema(),
            "/root/row",
            ReaderOptions::default(),
        )
        .unwrap();
        assert!(empty.next_batch().await.unwrap().is_none());
    });
    for path in [
        "root/row",
        "/root//row",
        "/root/@id",
        "/root/*",
        "/root/u:row",
    ] {
        assert!(matches!(
            XmlReader::new(Cursor::new(b""), schema(), path, ReaderOptions::default()),
            Err(ReadError::Configuration(_))
        ));
    }
    for options in [
        ReaderOptions {
            batch_size: 0,
            ..Default::default()
        },
        ReaderOptions {
            max_record_bytes: 0,
            ..Default::default()
        },
        ReaderOptions {
            max_depth: 65535,
            ..Default::default()
        },
        ReaderOptions {
            max_batch_bytes: 1,
            ..Default::default()
        },
    ] {
        let input = Input::new(b"<root/>", 1, false);
        let consumed = input.consumed.clone();
        assert!(XmlReader::new(input, schema(), "/root/row", options).is_err());
        assert_eq!(consumed.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn first_batch_does_not_read_the_tail_and_drop_releases_input() {
    block_on(async {
        let xml = format!(
            "<root><row id='1'/><skip>{}</skip></root>",
            "x".repeat(5000)
        );
        let input = Input::new(xml.as_bytes(), 5, false);
        let consumed = input.consumed.clone();
        let dropped = input.dropped.clone();
        let mut reader = XmlReader::new(
            input,
            schema(),
            "/root/row",
            ReaderOptions {
                batch_size: 1,
                ..Default::default()
            },
        )
        .unwrap();
        fn assert_send<T: Send>(_: &T) {}
        assert_send(&reader);
        assert_eq!(consumed.load(Ordering::SeqCst), 0);
        assert_eq!(reader.next_batch().await.unwrap().unwrap().num_rows(), 1);
        assert!(consumed.load(Ordering::SeqCst) < 30);
        drop(reader);
        assert!(dropped.load(Ordering::SeqCst));
    });
}
