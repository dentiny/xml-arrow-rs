use arrow_array::StringArray;
use arrow_schema::DataType;
use xml_arrow::{XmlBatchBuilder, XmlField, XmlSchema};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema = XmlSchema::new(vec![
        XmlField::new("id", "@id", DataType::Int64, false),
        XmlField::new("title", "title", DataType::Utf8, false),
        XmlField::new("price", "price", DataType::Float64, true),
    ])?;
    let mut builder = XmlBatchBuilder::new(schema, 1024)?;
    for record in [
        r#"<book id="1"><title>Rust &amp; Arrow</title><price>29.5</price></book>"#,
        r#"<book id="2"><title><![CDATA[XML <basics>]]></title></book>"#,
    ] {
        builder.append_xml(record)?;
    }
    if let Some(batch) = builder.flush()? {
        println!("{} rows, {} columns", batch.num_rows(), batch.num_columns());
        let titles = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        for title in titles.iter().flatten() {
            println!("{title}");
        }
    }
    Ok(())
}
