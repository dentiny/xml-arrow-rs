use std::path::Path;

use opendal::{services::Fs, Operator};
use xml_arrow::{InferenceOptions, NamespaceBinding, ReaderOptions, XmlReader};
use xml_io::file::XmlFile;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata")).canonicalize()?;
    let root = root.to_str().ok_or("fixture path must be UTF-8")?;
    let files = [
        XmlFile::from_uri(&format!("{root}/lancedb-pom.xml"))?,
        XmlFile::new(Operator::new(Fs::default().root(root))?, "lancedb-pom.xml")?,
    ];
    for file in files {
        let mut reader = XmlReader::infer(
            file.open().await?,
            "/m:project/m:dependencyManagement/m:dependencies/m:dependency",
            &[NamespaceBinding {
                prefix: "m",
                uri: "http://maven.apache.org/POM/4.0.0",
            }],
            ReaderOptions::default(),
            InferenceOptions::default(),
        )
        .await?;
        let mut rows = 0;
        while let Some(batch) = reader.next_batch().await? {
            rows += batch.num_rows();
        }
        assert_eq!(rows, 7);
        println!("{}: {rows} dependencies", file.source_uri());
    }
    Ok(())
}
