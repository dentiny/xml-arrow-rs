use std::{hint::black_box, time::Duration};

use arrow_schema::DataType::{Boolean, Float64, Int64, Utf8};
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use futures_executor::block_on;
use futures_util::io::Cursor;
use xml_arrow::{InferenceOptions, ReaderOptions, XmlField, XmlReader, XmlSchema};

struct Case {
    name: &'static str,
    record: String,
    rows: usize,
    schema: XmlSchema,
}

impl Case {
    fn bytes(&self) -> usize {
        "<root></root>".len() + self.record.len() * self.rows
    }

    fn document(&self) -> String {
        let mut xml = String::with_capacity(self.bytes());
        xml.push_str("<root>");
        for _ in 0..self.rows {
            xml.push_str(&self.record);
        }
        xml.push_str("</root>");
        xml
    }
}

fn cases() -> Vec<Case> {
    let mut wide = String::from("<row>");
    let mut fields = Vec::new();
    for index in 0..64 {
        wide.push_str(&format!("<v{index:02}>{index}</v{index:02}>"));
        fields.push(XmlField::new(
            format!("v{index:02}"),
            format!("v{index:02}"),
            Int64,
            true,
        ));
    }
    wide.push_str("</row>");
    vec![
        Case {
            name: "flat",
            record: "<row id='7'><title>Rust &amp; Arrow</title><active>true</active><price>29.5</price></row>".into(),
            rows: 10_000,
            schema: XmlSchema::new(vec![
                XmlField::new("@id", "@id", Int64, true),
                XmlField::new("active", "active", Boolean, true),
                XmlField::new("price", "price", Float64, true),
                XmlField::new("title", "title", Utf8, true),
            ]).unwrap(),
        },
        Case { name: "wide", record: wide, rows: 2_000, schema: XmlSchema::new(fields).unwrap() },
        Case {
            name: "nested",
            record: "<row id='7'><author id='3'><name>Jane</name></author><tag>Rust</tag><tag>Arrow</tag></row>".into(),
            rows: 5_000,
            schema: XmlSchema::new(vec![
                XmlField::new("@id", "@id", Int64, true),
                XmlField::structure("author", "author", true, vec![
                    XmlField::new("@id", "@id", Int64, true),
                    XmlField::new("name", "name", Utf8, true),
                ]),
                XmlField::list("tag", ".", true, XmlField::new("tag", "tag", Utf8, true)),
            ]).unwrap(),
        },
        Case {
            name: "large",
            record: format!("<row id='7'><payload>{}</payload></row>", "x".repeat(256 * 1024)),
            rows: 64,
            schema: XmlSchema::new(vec![
                XmlField::new("@id", "@id", Int64, true),
                XmlField::new("payload", "payload", Utf8, true),
            ]).unwrap(),
        },
    ]
}

fn readers(criterion: &mut Criterion) {
    let cases = cases();
    for (mode, infer) in [("explicit", false), ("infer", true)] {
        let mut group = criterion.benchmark_group(mode);
        for case in &cases {
            group.throughput(Throughput::Bytes(case.bytes() as u64));
            group.bench_function(case.name, |bencher| {
                // Build only the selected fixture, outside the timed iterations.
                let xml = case.document();
                bencher.iter(|| {
                    block_on(async {
                        let input = Cursor::new(black_box(xml.as_bytes()));
                        let mut reader = if infer {
                            XmlReader::infer(
                                input,
                                "/root/row",
                                &[],
                                ReaderOptions::default(),
                                InferenceOptions::default(),
                            )
                            .await
                            .unwrap()
                        } else {
                            XmlReader::new(
                                input,
                                case.schema.clone(),
                                "/root/row",
                                ReaderOptions::default(),
                            )
                            .unwrap()
                        };
                        assert_eq!(
                            reader.schema().as_ref(),
                            case.schema.arrow_schema().as_ref()
                        );
                        let mut rows = 0;
                        while let Some(batch) = reader.next_batch().await.unwrap() {
                            rows += batch.num_rows();
                            black_box(batch);
                        }
                        assert_eq!(rows, case.rows);
                    })
                });
            });
        }
        group.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    targets = readers
}
criterion_main!(benches);
