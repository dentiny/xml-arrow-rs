use arrow_array::{cast::AsArray, types::Int64Type};
use arrow_schema::DataType;
use futures_util::{io::Cursor, TryStreamExt};
use xml_arrow::{InferenceOptions, ReaderOptions, XmlReader};

const NUMBERS: &[u8] = include_bytes!("../../../testdata/numeric-boundaries.xml");

async fn infer(path: &str) -> XmlReader<'static> {
    XmlReader::infer(
        Cursor::new(NUMBERS),
        path,
        &[],
        ReaderOptions::default(),
        InferenceOptions::default(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn numeric_inference_preserves_large_integers() {
    for (path, values) in [
        ("/cases/positive/row", ["9007199254740993", "0.5"]),
        ("/cases/positive-reversed/row", ["0.5", "9007199254740993"]),
        ("/cases/maximum/row", ["9223372036854775807", "0.5"]),
        (
            "/cases/maximum-reversed/row",
            ["0.5", "9223372036854775807"],
        ),
        ("/cases/negative/row", ["-9007199254740993", "0.5"]),
        ("/cases/negative-reversed/row", ["0.5", "-9007199254740993"]),
    ] {
        let reader = infer(path).await;
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
    let mut reader = infer("/cases/integer-only/row").await;
    assert_eq!(reader.schema().field(0).data_type(), &DataType::Int64);
    assert_eq!(
        reader
            .next_batch()
            .await
            .unwrap()
            .unwrap()
            .column(0)
            .as_primitive::<Int64Type>()
            .values()
            .as_ref(),
        &[9007199254740993, 9223372036854775807, -9007199254740993, 1]
    );
    for path in ["/cases/safe-positive/row", "/cases/safe-minimum/row"] {
        assert_eq!(
            infer(path).await.schema().field(0).data_type(),
            &DataType::Float64
        );
    }
    for (path, integer) in [
        ("/cases/overflow-positive/row", "18446744073709551617"),
        ("/cases/overflow-negative/row", "-9223372036854775809"),
    ] {
        let mut reader = infer(path).await;
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
