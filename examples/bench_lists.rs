//! Compare direct-child and multi-segment list selection in a release build.
use arrow_schema::DataType::Utf8;
use std::{hint::black_box, time::Instant};
use xml_arrow::{XmlBatchBuilder, XmlField, XmlSchema};
fn main() {
    let size: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "1024".into())
        .parse()
        .unwrap();
    let nested = std::env::args().nth(2).as_deref() == Some("nested");
    let item = XmlField::new(
        "item",
        if nested { "group/item" } else { "item" },
        Utf8,
        false,
    );
    let schema = XmlSchema::new(vec![XmlField::list("values", ".", false, item)]).unwrap();
    let items = "<item>abcdefgh</item>".repeat(size);
    let input = if nested {
        format!("<r><group>{items}</group></r>")
    } else {
        format!("<r>{items}</r>")
    };
    let start = Instant::now();
    let mut rows = 0;
    while start.elapsed().as_secs_f64() < 2.0 {
        let mut b = XmlBatchBuilder::new(schema.clone(), 32).unwrap();
        for _ in 0..32 {
            b.append_xml(&input).unwrap();
        }
        let batch = b.flush().unwrap().unwrap();
        assert_eq!(batch.num_rows(), 32);
        black_box(batch);
        rows += 32;
    }
    println!(
        "{{\"case\":\"xml-list-{size}-{nested}\",\"rows\":{rows},\"ns_per_row\":{}}}",
        start.elapsed().as_nanos() as f64 / rows as f64
    );
}
