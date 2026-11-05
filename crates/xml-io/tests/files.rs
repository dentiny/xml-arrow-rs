use std::io;

use arrow_array::{cast::AsArray, types::Int64Type};
use arrow_schema::DataType;
use futures_util::io::{AsyncBufRead, AsyncReadExt};
use opendal::{services::Memory, Operator};
use tokio::fs;
use xml_arrow::{ReaderOptions, XmlField, XmlReader, XmlSchema};
use xml_io::file::XmlFile;

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
    fs::write(&path, XML).await.unwrap();
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
    assert!(!fs::try_exists(&parent).await.unwrap());
    assert!(bytes(&missing).await.is_err());
    assert!(!fs::try_exists(&parent).await.unwrap());
}

#[tokio::test]
async fn parent_segments_resolve_at_open_and_follow_symlinks() {
    let cwd = std::env::current_dir().unwrap();
    let directory = tempfile::tempdir_in(&cwd).unwrap();
    let child = directory.path().join("child");
    fs::create_dir(&child).await.unwrap();
    let path = child.join("../books.xml");
    let files = [
        XmlFile::from_uri(path.to_str().unwrap()).unwrap(),
        XmlFile::from_uri(path.strip_prefix(&cwd).unwrap().to_str().unwrap()).unwrap(),
    ];
    fs::write(directory.path().join("books.xml"), XML)
        .await
        .unwrap();
    for file in files {
        assert_eq!(ids(file.open().await.unwrap()).await, [1, 2]);
    }
    let missing = directory.path().join("missing");
    let file = XmlFile::from_uri(missing.join("../absent.xml").to_str().unwrap()).unwrap();
    assert!(bytes(&file).await.is_err());
    assert!(!fs::try_exists(&missing).await.unwrap());

    #[cfg(unix)]
    {
        let target = directory.path().join("target");
        fs::create_dir_all(target.join("child")).await.unwrap();
        fs::write(target.join("books.xml"), "<root><row id='3'/></root>")
            .await
            .unwrap();
        let link = directory.path().join("link");
        fs::symlink(target.join("child"), &link).await.unwrap();
        let file = XmlFile::from_uri(link.join("../books.xml").to_str().unwrap()).unwrap();
        assert_eq!(ids(file.open().await.unwrap()).await, [3]);
    }
}

#[tokio::test]
async fn configured_operator_preserves_its_root_and_returns_independent_streams() {
    let operator = Operator::new(Memory::default().root("project")).unwrap();
    let chunks: opendal::Buffer = [&XML[..17], &XML[17..23], &XML[23..]]
        .into_iter()
        .flat_map(opendal::Buffer::from)
        .collect();
    operator.write("data.xml", chunks).await.unwrap();
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
