use arrow_array::{cast::AsArray, types::Int64Type};
use arrow_schema::DataType::{Int64, Utf8};
use futures_util::io::Cursor;
use xml_arrow::{NamespaceBinding, ReaderOptions, XmlField, XmlReader, XmlSchema};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema = XmlSchema::with_namespaces(
        vec![
            XmlField::new("id", "@id", Int64, false),
            XmlField::new("title", "b:title", Utf8, false),
        ],
        &[NamespaceBinding {
            prefix: "b",
            uri: "urn:books",
        }],
    )?;
    let input = Cursor::new(
        br#"<catalog xmlns="urn:books">
        <book id="1"><title>Rust &amp; Arrow</title></book>
        <book id="2"><title>XML</title></book>
    </catalog>"#,
    );
    let mut reader = XmlReader::new(
        input,
        schema,
        "/b:catalog/b:book",
        ReaderOptions {
            batch_size: 1,
            projection: Some(vec![1, 0]),
            ..Default::default()
        },
    )?;
    futures_executor::block_on(async {
        while let Some(batch) = reader.next_batch().await? {
            let title = batch.column(0).as_string::<i32>().value(0);
            let id = batch.column(1).as_primitive::<Int64Type>().value(0);
            println!("{id}: {title}");
        }
        xml_arrow::ReaderResult::Ok(())
    })?;
    Ok(())
}
