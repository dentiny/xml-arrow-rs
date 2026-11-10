//! Run cargo bench -p xml-arrow --bench nested_struct -- [rows] [rounds].
use arrow_schema::DataType;
use std::{hint::black_box, time::Instant};
use xml_arrow::{XmlBatchBuilder, XmlField, XmlSchema};
fn main() {
    let args: Vec<_> = std::env::args()
        .skip(1)
        .filter(|arg| arg != "--bench")
        .collect();
    let number =
        |i: usize, default: usize| args.get(i).map(|x| x.parse().unwrap()).unwrap_or(default);
    let rows = number(0, 20_000);
    let rounds = number(1, 15);
    assert!(rows > 0 && rounds > 0);
    let input = format!(
        "<row><a><b id=\"42\"><text>{}</text></b></a></row>",
        "payload 你好 ".repeat(8)
    );
    let mut children = vec![
        XmlField::new("id", "@id", DataType::Int64, false),
        XmlField::new("text", "text", DataType::Utf8, false),
    ];
    for i in 0..32 {
        children.push(XmlField::new(
            format!("optional{i}"),
            format!("@optional{i}"),
            DataType::Utf8,
            true,
        ));
    }
    let schema = XmlSchema::new(vec![XmlField::structure(
        "a",
        "a",
        false,
        vec![XmlField::structure("b", "b", false, children)],
    )])
    .unwrap();
    for iteration in 0..=rounds {
        let start = Instant::now();
        let mut builder = XmlBatchBuilder::new(schema.clone(), 1024).unwrap();
        let mut count = 0;
        for row in 0..rows {
            builder.append_xml(&input).unwrap();
            if (row + 1) % 1024 == 0 {
                let batch = builder.flush().unwrap().unwrap();
                count += batch.num_rows();
                black_box(batch);
            }
        }
        if let Some(batch) = builder.flush().unwrap() {
            count += batch.num_rows();
            black_box(batch);
        }
        assert_eq!(count, rows);
        if iteration > 0 {
            println!(
                "iteration={iteration},rows={rows},seconds={:.9}",
                start.elapsed().as_secs_f64()
            );
        }
    }
}
