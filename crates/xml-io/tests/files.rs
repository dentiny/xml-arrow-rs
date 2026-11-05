use std::io;

use arrow_array::{cast::AsArray, types::Int64Type};
use arrow_schema::DataType;
use futures_util::io::{AsyncBufRead, AsyncReadExt};
use xml_arrow::{ReaderOptions, XmlField, XmlReader, XmlSchema};
use xml_io::{
    opendal::{services::Memory, Operator},
    XmlFile,
};

const XML: &str = "<root><row id='1'/><row id='2'/></root>";

async fn bytes(file: &XmlFile) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.open().await?.read_to_end(&mut bytes).await?;
    Ok(bytes)
}

async fn ids(input: impl AsyncBufRead + Unpin + Send) -> Vec<i64> {
    let schema = XmlSchema::new(vec![XmlField::new("id", "@id", DataType::Int64, false)]).unwrap();
    let mut reader = XmlReader::new(
        input,
        schema,
        "/root/row",
        ReaderOptions {
            batch_size: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let mut ids = Vec::new();
    while let Some(batch) = reader.next_batch().await.unwrap() {
        ids.extend_from_slice(batch.column(0).as_primitive::<Int64Type>().values());
    }
    ids
}

#[tokio::test]
async fn local_paths_and_file_urls_reopen_without_creating_missing_directories() {
    let cwd = std::env::current_dir().unwrap();
    let directory = tempfile::tempdir_in(&cwd).unwrap();
    let path = directory.path().join("books #北京.xml");
    std::fs::write(&path, XML).unwrap();
    for uri in [
        path.to_str().unwrap().to_owned(),
        path.strip_prefix(&cwd)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned(),
        url::Url::from_file_path(&path).unwrap().to_string(),
    ] {
        let file = XmlFile::from_uri(&uri).unwrap();
        assert_eq!(file.source_uri(), path.to_str().unwrap());
        let mut prefix = [0; 5];
        file.open()
            .await
            .unwrap()
            .read_exact(&mut prefix)
            .await
            .unwrap();
        assert_eq!(&prefix, b"<root");
        assert_eq!(ids(file.open().await.unwrap()).await, [1, 2]);
    }
    let parent = directory.path().join("missing");
    let missing = XmlFile::from_uri(parent.join("data.xml").to_str().unwrap()).unwrap();
    assert!(!parent.exists());
    assert!(bytes(&missing).await.is_err());
    assert!(!parent.exists());
}

#[tokio::test]
async fn configured_operator_preserves_its_root_and_returns_independent_streams() {
    let operator = Operator::new(Memory::default().root("project")).unwrap();
    operator.write("data.xml", XML).await.unwrap();
    let file = XmlFile::new(operator.clone(), "data.xml").unwrap();
    assert_eq!(file.source_uri(), "data.xml");
    let mut first = file.open().await.unwrap();
    first.read_exact(&mut [0; 3]).await.unwrap();
    assert_eq!(bytes(&file.clone()).await.unwrap(), XML.as_bytes());
    let mut rest = String::new();
    first.read_to_string(&mut rest).await.unwrap();
    assert_eq!(rest, &XML[3..]);
    assert_eq!(ids(file.open().await.unwrap()).await, [1, 2]);
    assert!(
        bytes(&XmlFile::new(operator.clone(), "missing.xml").unwrap())
            .await
            .is_err()
    );
    for path in ["", "directory/"] {
        assert_eq!(
            XmlFile::new(operator.clone(), path).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
    for uri in ["", "s3://bucket/data.xml", "file:///"] {
        assert_eq!(
            XmlFile::from_uri(uri).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

#[cfg(feature = "gzip")]
#[tokio::test]
async fn gzip_handles_single_and_concatenated_members_and_rejects_truncation() {
    fn member(bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }
    let operator = Operator::new(Memory::default()).unwrap();
    let single = member(XML.as_bytes());
    let split = XML.len() / 2;
    let multiple = [
        member(&XML.as_bytes()[..split]),
        member(b""),
        member(&XML.as_bytes()[split..]),
    ]
    .concat();
    for (path, compressed) in [
        ("single.xml.gz", single.clone()),
        ("members.bin", multiple.clone()),
    ] {
        operator.write(path, compressed.clone()).await.unwrap();
        let file = XmlFile::new(operator.clone(), path).unwrap();
        assert_eq!(bytes(&file).await.unwrap(), compressed);
        assert_eq!(ids(file.open_gzip().await.unwrap()).await, [1, 2]);
        // The returned reader owns its input and can outlive the file handle.
        let input = file.open_gzip().await.unwrap();
        drop(file);
        assert_eq!(ids(input).await, [1, 2]);
    }
    for compressed in [&single, &multiple] {
        operator
            .write("truncated.gz", compressed[..compressed.len() - 1].to_vec())
            .await
            .unwrap();
        let file = XmlFile::new(operator.clone(), "truncated.gz").unwrap();
        let mut input = file.open_gzip().await.unwrap();
        assert!(input.read_to_end(&mut Vec::new()).await.is_err());
    }
}
