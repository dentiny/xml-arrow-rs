use std::{io, task::Poll};

use arrow_array::{cast::AsArray, types::Int64Type, RecordBatch};
use arrow_schema::DataType::{Int64, Utf8};
use futures_util::{
    io::{AsyncBufRead, AsyncReadExt, BufReader, Cursor},
    stream, FutureExt, TryStreamExt,
};
use xml_arrow::{
    Limit, NamespaceBinding, ReadError, ReaderOptions, XmlField, XmlReader, XmlSchema,
};

fn schema() -> XmlSchema {
    XmlSchema::new(vec![XmlField::new("id", "@id", Int64, false)]).unwrap()
}

// Force cancellation inside XML tokens.
fn input(mut bytes: &[u8]) -> impl AsyncBufRead + Unpin + Send + '_ {
    let mut pending = false;
    stream::poll_fn(move |cx| {
        pending = !pending;
        if pending {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else if bytes.is_empty() {
            Poll::Ready(None)
        } else {
            let (byte, rest) = bytes.split_at(1);
            bytes = rest;
            Poll::Ready(Some(Ok::<_, io::Error>(byte)))
        }
    })
    .into_async_read()
}

fn read<'a>(input: impl AsyncBufRead + Unpin + Send + 'a, options: ReaderOptions) -> XmlReader<'a> {
    XmlReader::new(input, schema(), "/root/row", options).unwrap()
}

fn ids(batch: &RecordBatch, column: usize) -> &[i64] {
    batch.column(column).as_primitive::<Int64Type>().values()
}

#[tokio::test]
async fn split_tokens_utf8_and_namespace_context_survive_batch_boundaries() {
    let schema = XmlSchema::with_namespaces(
        vec![
            XmlField::new("id", "@id", Int64, false),
            XmlField::new("name", "r:title", Utf8, true),
        ],
        &[NamespaceBinding {
            prefix: "r",
            uri: "urn:rows&records",
        }],
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
    let names = first.column(0).as_string::<i32>();
    assert_eq!(
        names.iter().collect::<Vec<_>>(),
        [Some("北<京>&"), Some("東京")]
    );
    assert_eq!(ids(&first, 1), &[1, 2]);
    let rest: Vec<_> = reader.into_stream().try_collect().await.unwrap();
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].num_rows(), 1);
    assert_eq!(rest[0].column(0).null_count(), 1);
    assert_eq!(ids(&rest[0], 1), &[3]);
}

#[test]
fn cancelled_next_batch_resumes_without_losing_partial_events_or_rows() {
    for (batch_size, bom) in [(1, "\u{FEFF}"), (1024, "")] {
        let xml = format!("{bom}<root><row id='1'/><row id='2' note='北😀'/></root>");
        let input = input(xml.as_bytes());
        let mut reader = read(
            input,
            ReaderOptions {
                batch_size,
                ..Default::default()
            },
        );
        let mut cancelled = 0;
        let mut rows = Vec::new();
        let mut eof = false;
        for _ in 0..10000 {
            match reader.next_batch().now_or_never() {
                None => cancelled += 1,
                Some(Ok(Some(batch))) => rows.extend_from_slice(ids(&batch, 0)),
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
}

#[tokio::test]
async fn conversion_and_io_errors_end_the_reader() {
    let xml = "<root>\n  <row id='1'/>\n  <row id='bad'/>\n  <row id='3'/>\n</root>";
    // XML, path, record index, opening-tag marker.
    for (xml, path, index, tag) in [
        (xml, "/root/row", 2, "<row id='bad'"),
        ("\u{FEFF}<row id='bad'/>", "/row", 1, "<row"),
        (
            r#"<root xmlns:p='urn:x" id="7'><row/></root>"#,
            "/root/row",
            1,
            "<row",
        ),
    ] {
        let mut reader =
            XmlReader::new(input(xml.as_bytes()), schema(), path, Default::default()).unwrap();
        let offset = xml.find(tag).unwrap() as u64;
        assert!(
            matches!(reader.next_batch().await, Err(ReadError::Conversion {
                record_index, byte_offset, source: xml_arrow::Error::Field { name, .. },
            }) if record_index == index && byte_offset == offset && name == "id")
        );
        for _ in 0..2 {
            assert!(reader.next_batch().await.unwrap().is_none());
        }
    }
    let prefix = &xml.as_bytes()[..xml.find("<row id='bad'").unwrap()];
    let error = stream::iter([Err::<&[u8], _>(io::Error::other("injected input failure"))])
        .into_async_read();
    let mut reader = read(Cursor::new(prefix).chain(error), Default::default());
    let error = reader.next_batch().await.unwrap_err();
    assert!(error.to_string().contains("injected input failure"));
    assert!(std::error::Error::source(&error).is_some());
    assert!(reader.next_batch().await.unwrap().is_none());
}

#[tokio::test]
async fn limits_apply_to_unselected_content_and_batches_keep_lookahead_records() {
    for (xml, expected) in [
        (
            "<root><skip><deep><too-deep/></deep></skip></root>".to_owned(),
            Limit::Depth,
        ),
        (
            format!("<root><!--{}--></root>", "x".repeat(5000)),
            Limit::EventBytes,
        ),
        (
            "<root><row><v>123456789012345</v><v>123456789012345</v></row></root>".to_owned(),
            Limit::RecordBytes,
        ),
    ] {
        let mut source = Cursor::new(xml.as_bytes());
        let mut reader = read(
            BufReader::with_capacity(1, &mut source),
            ReaderOptions {
                max_depth: 3,
                max_record_bytes: 32,
                max_batch_bytes: 64,
                ..Default::default()
            },
        );
        assert!(
            matches!(reader.next_batch().await, Err(ReadError::LimitExceeded { kind, .. }) if kind == expected)
        );
        assert!(reader.next_batch().await.unwrap().is_none());
        drop(reader);
        if expected == Limit::EventBytes {
            assert!(source.position() < 100);
        }
    }
    let options = ReaderOptions {
        batch_size: 10,
        max_record_bytes: 8,
        max_batch_bytes: 13,
        projection: Some(vec![]),
        ..Default::default()
    };
    let mut reader = read(
        Cursor::new(b"\xEF\xBB\xBF<root><row/><row/><row/></root>"),
        options,
    );
    let first = reader.next_batch().await.unwrap().unwrap();
    assert_eq!((first.num_rows(), first.num_columns()), (2, 0));
    assert_eq!(reader.next_batch().await.unwrap().unwrap().num_rows(), 1);
    assert!(reader.next_batch().await.unwrap().is_none());
}

#[tokio::test]
async fn malformed_documents_fail_and_configuration_is_checked_before_io() {
    for xml in [
        "",
        "<root><row/></root",
        "<root/><root/>",
        "\u{FEFF}\u{FEFF}<root/>",
        "<!DOCTYPE root><root/>",
        "<root><row id='1' a='1' a='2'/></root>",
        "<root><row id='1'><1bad/></row></root>",
        "<root><row id='1'>&unknown;</row></root>",
    ] {
        let mut reader = read(Cursor::new(xml.as_bytes()), Default::default());
        assert!(reader.next_batch().await.is_err(), "{xml}");
        assert!(reader.next_batch().await.unwrap().is_none());
    }
    let mut empty = read(
        Cursor::new("\u{FEFF}<?xml version='1.0'?><root/>".as_bytes()),
        Default::default(),
    );
    assert!(empty.next_batch().await.unwrap().is_none());
    let paths = [
        "root/row",
        "/root//row",
        "/root/@id",
        "/root/*",
        "/root/u:row",
    ]
    .map(|path| (path, ReaderOptions::default()));
    let options = [
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
    ]
    .map(|options| ("/root/row", options));
    for (path, options) in paths.into_iter().chain(options) {
        let mut source = Cursor::new(b"<root/>");
        assert!(matches!(
            XmlReader::new(&mut source, schema(), path, options),
            Err(ReadError::Configuration(_))
        ));
        assert_eq!(source.position(), 0);
    }
}

#[tokio::test]
async fn first_batch_does_not_read_the_tail_and_drop_releases_input() {
    let xml = format!(
        "<root><row id='1'/><skip>{}</skip></root>",
        "x".repeat(5000)
    );
    let mut source = Cursor::new(xml.as_bytes());
    let options = ReaderOptions {
        batch_size: 1,
        ..Default::default()
    };
    drop(read(
        BufReader::with_capacity(1, &mut source),
        options.clone(),
    ));
    assert_eq!(source.position(), 0);
    let mut reader = read(BufReader::with_capacity(1, &mut source), options);
    fn assert_send<T: Send>(_: &T) {}
    assert_send(&reader);
    assert_eq!(reader.next_batch().await.unwrap().unwrap().num_rows(), 1);
    drop(reader);
    assert!(source.position() < 30);
}

#[tokio::test]
async fn skipped_content_is_not_reparsed() {
    let xml = b"<root><skip a='1' a='2'>&unknown;</skip><row id='7'/></root>";
    let mut reader = read(input(xml), ReaderOptions::default());
    let batch = reader.next_batch().await.unwrap().unwrap();
    assert_eq!(ids(&batch, 0), &[7]);
    assert!(reader.next_batch().await.unwrap().is_none());
}
