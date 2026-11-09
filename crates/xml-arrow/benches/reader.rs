//! Run with cargo bench -p xml-arrow --bench reader.
use arrow_schema::DataType;
use futures_util::io::Cursor;
use std::{hint::black_box, time::Instant};
use xml_arrow::{ReaderOptions, XmlField, XmlReader, XmlSchema};
fn main() {
    let xml = format!(
        "<rows>{}</rows>",
        "<row><id>42</id><text>hello &amp; 你好</text></row>".repeat(20000)
    );
    let schema = XmlSchema::new(vec![
        XmlField::new("id", "id", DataType::Int64, false),
        XmlField::new("text", "text", DataType::Utf8, false),
    ])
    .unwrap();
    for projected in [false, true] {
        for iteration in 0..8 {
            let start = Instant::now();
            futures_executor::block_on(async {
                let options = ReaderOptions {
                    batch_size: 1024,
                    projection: projected.then(|| vec![0]),
                    ..Default::default()
                };
                let mut reader = XmlReader::new(
                    Cursor::new(xml.as_bytes()),
                    schema.clone(),
                    "/rows/row",
                    options,
                )
                .unwrap();
                let mut rows = 0;
                while let Some(batch) = reader.next_batch().await.unwrap() {
                    rows += black_box(batch).num_rows();
                }
                assert_eq!(rows, 20000);
            });
            if iteration > 0 {
                println!(
                    "projected={projected} seconds={:.6}",
                    start.elapsed().as_secs_f64()
                );
            }
        }
    }
}
