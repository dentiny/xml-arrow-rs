use std::{hint::black_box, time::Instant};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let iterations: usize = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(10);
    let n = 32768;
    let input = "<row><id>42</id><text>benchmark payload</text></row>";
    let schema = xml_arrow::XmlSchema::new(vec![
        xml_arrow::XmlField::new("id", "id", arrow_schema::DataType::Int64, false),
        xml_arrow::XmlField::new("text", "text", arrow_schema::DataType::Utf8, false),
    ])
    .unwrap();
    let start = Instant::now();
    for _ in 0..iterations {
        let mut builder = xml_arrow::XmlBatchBuilder::new(schema.clone(), 1024).unwrap();
        for i in 0..n {
            builder.append_xml(input).unwrap();
            if (i + 1) % 1024 == 0 {
                black_box(builder.flush().unwrap());
            }
        }
    }
    println!("{{\"case\":\"xml-native\",\"iterations\":{},\"input_bytes\":{},\"units\":{},\"seconds\":{}}}",iterations,input.len()*n,n*iterations,start.elapsed().as_secs_f64());
}
