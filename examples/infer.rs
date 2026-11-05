use futures_util::{io::Cursor, TryStreamExt};
use xml_arrow::{InferenceOptions, ReaderOptions, XmlReader};

fn main() -> xml_arrow::ReaderResult<()> {
    futures_executor::block_on(async {
        let input = Cursor::new(
            b"<rows><row id='1'><price>2</price></row><row id='2'><price>2.5</price></row></rows>",
        );
        let reader = XmlReader::infer(
            input,
            "/rows/row",
            &[],
            ReaderOptions::default(),
            InferenceOptions::default(),
        )
        .await?;
        println!("{}", reader.schema());
        let batches: Vec<_> = reader.into_stream().try_collect().await?;
        assert_eq!(batches[0].num_rows(), 2);
        Ok(())
    })
}
