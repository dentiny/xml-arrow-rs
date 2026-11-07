use opendal::{services::Fs, Operator};
use xml_arrow::{InferenceOptions, NamespaceBinding, ReaderOptions, XmlReader};
use xml_io::file::XmlFile;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../xml-arrow/testdata");
    let files = [
        XmlFile::from_uri(&format!("{root}/catalog.xml"))?,
        XmlFile::new(Operator::new(Fs::default().root(root))?, "catalog.xml")?,
    ];
    for file in files {
        let mut reader = XmlReader::infer(
            file.open().await?,
            "/c:catalog/c:product",
            &[NamespaceBinding {
                prefix: "c",
                uri: "urn:open-arrow-access:catalog",
            }],
            ReaderOptions::default(),
            InferenceOptions::default(),
        )
        .await?;
        let mut rows = 0;
        while let Some(batch) = reader.next_batch().await? {
            rows += batch.num_rows();
        }
        assert_eq!(rows, 5);
        println!("{}: {rows} products", file.source_uri());
    }
    Ok(())
}
