//! A small in-memory reader throughput check; generation is outside the timer.
use arrow_schema::DataType;
use futures_executor::block_on;
use futures_util::io::Cursor;
use std::{hint::black_box, time::Instant};
use xml_arrow::{ReaderOptions, XmlField, XmlReader, XmlSchema};

fn main() {
    let schema = XmlSchema::new(vec![XmlField::new("id", "@id", DataType::Int64, false)]).unwrap();
    for (name, record, count) in [
        ("small records", "<row id='1'/>", 100_000),
        (
            "nested records",
            "<row id='1'><a><b>text &amp; more</b></a><c/></row>",
            100_000,
        ),
        (
            "skipped content",
            "<skip><a>text &amp; more</a><b/></skip>",
            100_000,
        ),
    ] {
        let xml = format!("<root>{}</root>", record.repeat(count));
        let mut elapsed = Vec::new();
        for _ in 0..5 {
            let started = Instant::now();
            let rows = block_on(async {
                let mut reader = XmlReader::new(
                    Cursor::new(xml.as_bytes()),
                    schema.clone(),
                    "/root/row",
                    ReaderOptions::default(),
                )
                .unwrap();
                let mut rows = 0;
                while let Some(batch) = reader.next_batch().await.unwrap() {
                    rows += black_box(batch.num_rows());
                }
                rows
            });
            assert_eq!(rows, if name == "skipped content" { 0 } else { count });
            elapsed.push(started.elapsed().as_secs_f64());
        }
        elapsed.sort_by(f64::total_cmp);
        println!(
            "{name}: {:.2} MiB/s, {:.3} s median",
            xml.len() as f64 / 1048576.0 / elapsed[2],
            elapsed[2]
        );
    }
}
