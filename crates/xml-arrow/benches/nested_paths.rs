//! Isolate nested field selection from streaming XML record extraction.
use arrow_schema::DataType::{Int64, Utf8};
use std::{hint::black_box, time::Instant};
use xml_arrow::{XmlBatchBuilder, XmlField, XmlSchema};
fn main() {
    let iterations: usize = std::env::args()
        .nth(1)
        .map(|s| s.parse().unwrap())
        .unwrap_or(100_000);
    let schema = XmlSchema::new(vec![
        XmlField::new("id", "a/b/id", Int64, false),
        XmlField::new("text", "a/b/text", Utf8, false),
    ])
    .unwrap();
    let xml = format!(
        "<row><a><b><id>42</id><text>{}</text></b></a></row>",
        "x".repeat(128)
    );
    let mut builder = XmlBatchBuilder::new(schema, 1024).unwrap();
    builder.append_xml(&xml).unwrap();
    black_box(builder.flush().unwrap());
    let start = Instant::now();
    for _ in 0..iterations {
        builder.append_xml(black_box(&xml)).unwrap();
        if builder.is_full() {
            black_box(builder.flush().unwrap());
        }
    }
    black_box(builder.flush().unwrap());
    println!(
        "rows={iterations} seconds={:.6}",
        start.elapsed().as_secs_f64()
    );
}
