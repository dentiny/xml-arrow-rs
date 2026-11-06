use std::io;

use arrow_array::{
    cast::AsArray,
    types::{Float64Type, Int64Type},
    Array,
};
use arrow_schema::DataType;
use futures_executor::block_on;
use futures_util::{
    io::{AsyncBufRead, Cursor},
    stream, TryStreamExt,
};
use xml_arrow::{InferenceOptions, Limit, NamespaceBinding, ReadError, ReaderOptions, XmlReader};

fn input(xml: &str) -> impl AsyncBufRead + Unpin + Send + '_ {
    stream::iter(xml.as_bytes().chunks(1).map(Ok::<_, io::Error>)).into_async_read()
}

#[test]
fn merging_is_order_independent_and_sampled_records_are_replayed() {
    block_on(async {
        let rows = [
            "<row id='1'><price>2</price><flag>true</flag><empty/><value/></row>",
            "<row id='2'><price>2.5</price><flag>false</flag><value>3</value><extra>東京</extra></row>",
        ];
        let mut schemas = Vec::new();
        for rows in [rows, [rows[1], rows[0]]] {
            let xml = format!("<root>{}{}</root>", rows[0], rows[1]);
            let reader = XmlReader::infer(
                input(&xml),
                "/root/row",
                &[],
                ReaderOptions::default(),
                InferenceOptions::default(),
            )
            .await
            .unwrap();
            let schema = reader.schema();
            assert!(schema.fields().iter().all(|field| field.is_nullable()));
            assert_eq!(
                schema.field_with_name("price").unwrap().data_type(),
                &DataType::Float64
            );
            assert_eq!(
                schema.field_with_name("flag").unwrap().data_type(),
                &DataType::Boolean
            );
            assert_eq!(
                schema.field_with_name("value").unwrap().data_type(),
                &DataType::Utf8
            );
            let batches: Vec<_> = reader.into_stream().try_collect().await.unwrap();
            assert_eq!(batches.len(), 1);
            assert_eq!(batches[0].num_rows(), 2);
            let ids = batches[0]
                .column_by_name("@id")
                .unwrap()
                .as_primitive::<Int64Type>();
            assert_eq!(
                ids.values().as_ref(),
                if rows[0].contains("id='1'") {
                    &[1, 2]
                } else {
                    &[2, 1]
                }
            );
            let prices = batches[0]
                .column_by_name("price")
                .unwrap()
                .as_primitive::<Float64Type>();
            assert_eq!(prices.values().iter().sum::<f64>(), 4.5);
            assert_eq!(batches[0].column_by_name("empty").unwrap().null_count(), 1);
            schemas.push(schema);
        }
        assert_eq!(schemas[0], schemas[1]);
    });
}

#[test]
fn namespaces_nested_lists_and_nil_match_the_existing_converter() {
    block_on(async {
        let xml = r#"<root xmlns='urn:rows' xmlns:n='http://www.w3.org/2001/XMLSchema-instance'>
            <row id='1'><author/><tag n:nil='true'/><tag>a</tag></row>
            <r:row xmlns:r='urn:rows' id='2'><r:author id='7'>Jane</r:author><r:tag>b</r:tag></r:row>
            <row id='unknown' n:nil='true'/>
        </root>"#;
        let reader = XmlReader::infer(
            input(xml),
            "/r:root/r:row",
            &[NamespaceBinding {
                prefix: "r",
                uri: "urn:rows",
            }],
            ReaderOptions::default(),
            InferenceOptions::default(),
        )
        .await
        .unwrap();
        let batches: Vec<_> = reader.into_stream().try_collect().await.unwrap();
        assert_eq!(
            batches[0]
                .column_by_name("@id")
                .unwrap()
                .as_string::<i32>()
                .value(2),
            "unknown"
        );
        let author = batches[0].column_by_name("r:author").unwrap().as_struct();
        assert_eq!(
            author
                .column_by_name("@id")
                .unwrap()
                .as_primitive::<Int64Type>()
                .iter()
                .collect::<Vec<_>>(),
            [None, Some(7), None]
        );
        assert_eq!(
            author
                .column_by_name(".")
                .unwrap()
                .as_string::<i32>()
                .iter()
                .collect::<Vec<_>>(),
            [Some(""), Some("Jane"), None]
        );
        let tags = batches[0].column_by_name("r:tag").unwrap().as_list::<i32>();
        assert_eq!(tags.value_offsets(), &[0, 2, 3, 3]);
        assert_eq!(
            tags.values().as_string::<i32>().iter().collect::<Vec<_>>(),
            [None, Some("a"), Some("b")]
        );

        let reader = XmlReader::infer(
            input("<root><row><x:v xmlns:x='urn:new'>1</x:v></row></root>"),
            "/root/row",
            &[],
            ReaderOptions::default(),
            InferenceOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(reader.schema().field(0).name(), "ns0:v");
        assert_eq!(
            reader.into_stream().try_collect::<Vec<_>>().await.unwrap()[0]
                .column(0)
                .as_primitive::<Int64Type>()
                .value(0),
            1
        );
    });
}

#[test]
fn row_and_byte_limits_stop_sampling_and_preserve_lookahead_errors() {
    block_on(async {
        let first = "<row><v>1</v></row>";
        let second = "<row><v>2.5</v></row>";
        let xml = format!("<root>{first}{second}<row><v>ignored</v></row></root>");
        for sampling in [
            InferenceOptions {
                max_records: 1,
                ..Default::default()
            },
            InferenceOptions {
                max_bytes: first.len() + second.len() - 1,
                ..Default::default()
            },
            InferenceOptions {
                max_bytes: first.len(),
                ..Default::default()
            },
        ] {
            let mut reader = XmlReader::infer(
                input(&xml),
                "/root/row",
                &[],
                ReaderOptions {
                    batch_size: 1,
                    ..Default::default()
                },
                sampling,
            )
            .await
            .unwrap();
            assert_eq!(reader.schema().field(0).data_type(), &DataType::Int64);
            assert_eq!(
                reader
                    .next_batch()
                    .await
                    .unwrap()
                    .unwrap()
                    .column(0)
                    .as_primitive::<Int64Type>()
                    .value(0),
                1
            );
            assert!(
                matches!(reader.next_batch().await, Err(ReadError::Conversion { record_index: 2, byte_offset, .. }) if byte_offset == (6 + first.len()) as u64)
            );
            assert!(reader.next_batch().await.unwrap().is_none());
        }
        let xml = "<root><row id='1'/><row id='2' new='3'/></root>";
        let reader = XmlReader::infer(
            input(xml),
            "/root/row",
            &[],
            ReaderOptions {
                projection: Some(vec![0]),
                ..Default::default()
            },
            InferenceOptions {
                max_records: 1,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(reader.schema().fields().len(), 1);
        assert_eq!(
            reader.into_stream().try_collect::<Vec<_>>().await.unwrap()[0].num_rows(),
            2
        );
    });
}

#[test]
fn invalid_options_fail_before_io_and_invalid_samples_return_errors() {
    block_on(async {
        for sampling in [
            InferenceOptions {
                max_records: 0,
                ..Default::default()
            },
            InferenceOptions {
                max_bytes: 0,
                ..Default::default()
            },
        ] {
            let mut source = Cursor::new(b"<root/>");
            assert!(matches!(
                XmlReader::infer(
                    &mut source,
                    "/root/row",
                    &[],
                    ReaderOptions::default(),
                    sampling
                )
                .await,
                Err(ReadError::Configuration(_))
            ));
            assert_eq!(source.position(), 0);
        }
        let result = XmlReader::infer(
            input("<root><row/></root>"),
            "/root/row",
            &[],
            ReaderOptions::default(),
            InferenceOptions {
                max_bytes: 1,
                ..Default::default()
            },
        )
        .await;
        assert!(matches!(
            result,
            Err(ReadError::LimitExceeded {
                kind: Limit::SampleBytes,
                ..
            })
        ));
        for xml in [
            "<root><row><v>text<child/></v></row></root>",
            "<root><row><v>1</v></row><row><v><child/></v></row></root>",
            "<root><row xmlns:n='http://www.w3.org/2001/XMLSchema-instance'><v n:nil='bad'/></row></root>",
            "<root><row/>",
        ] {
            assert!(XmlReader::infer(input(xml), "/root/row", &[], ReaderOptions::default(), InferenceOptions::default()).await.is_err(), "{xml}");
        }
        let mut reader = XmlReader::infer(
            input("<root/>"),
            "/root/row",
            &[],
            ReaderOptions::default(),
            InferenceOptions::default(),
        )
        .await
        .unwrap();
        assert!(reader.schema().fields().is_empty());
        assert!(reader.next_batch().await.unwrap().is_none());
    });
}

#[tokio::test]
async fn numeric_inference_preserves_large_integers() {
    for integer in [
        "9007199254740993",
        "9223372036854775807",
        "-9007199254740993",
    ] {
        for values in [[integer, "0.5"], ["0.5", integer]] {
            let xml = format!(
                "<root><row v='{}'/><row v='{}'/></root>",
                values[0], values[1]
            );
            let reader = XmlReader::infer(
                input(&xml),
                "/root/row",
                &[],
                ReaderOptions::default(),
                InferenceOptions::default(),
            )
            .await
            .unwrap();
            assert_eq!(reader.schema().field(0).data_type(), &DataType::Utf8);
            let batches = reader.into_stream().try_collect::<Vec<_>>().await.unwrap();
            assert_eq!(
                batches[0]
                    .column(0)
                    .as_string::<i32>()
                    .iter()
                    .collect::<Vec<_>>(),
                values.map(Some)
            );
        }
        let xml = format!("<root><row v='{integer}'/><row v='1'/></root>");
        let mut reader = XmlReader::infer(
            input(&xml),
            "/root/row",
            &[],
            ReaderOptions::default(),
            InferenceOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(reader.schema().field(0).data_type(), &DataType::Int64);
        assert_eq!(
            reader
                .next_batch()
                .await
                .unwrap()
                .unwrap()
                .column(0)
                .as_primitive::<Int64Type>()
                .value(0),
            integer.parse::<i64>().unwrap()
        );
    }
    for integer in ["9007199254740994", "-9223372036854775808"] {
        let xml = format!("<root><row><v>{integer}</v></row><row><v>0.5</v></row></root>");
        let reader = XmlReader::infer(
            input(&xml),
            "/root/row",
            &[],
            ReaderOptions::default(),
            InferenceOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(reader.schema().field(0).data_type(), &DataType::Float64);
    }
    for integer in ["18446744073709551617", "-9223372036854775809"] {
        let xml = format!("<root><row><v>{integer}</v></row></root>");
        let mut reader = XmlReader::infer(
            input(&xml),
            "/root/row",
            &[],
            ReaderOptions::default(),
            InferenceOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(reader.schema().field(0).data_type(), &DataType::Utf8);
        assert_eq!(
            reader
                .next_batch()
                .await
                .unwrap()
                .unwrap()
                .column(0)
                .as_string::<i32>()
                .value(0),
            integer
        );
    }
}
