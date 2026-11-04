use arrow_schema::DataType::{Binary, Int64, Utf8};
use xml_arrow::{XmlBatchBuilder, XmlField, XmlSchema};

#[test]
fn rejects_invalid_schema() {
    for field in [
        XmlField::new("value", "/value", Utf8, true),
        XmlField::new("value", "value", Binary, true),
        XmlField::new("", "value", Utf8, true),
    ] {
        assert!(XmlSchema::new(vec![field]).is_err());
    }
    assert!(XmlSchema::new(vec![
        XmlField::new("id", "@id", Int64, false),
        XmlField::new("id", "other", Utf8, true),
    ])
    .is_err());
}

#[test]
fn rejects_invalid_batch_size_and_projection() {
    let schema = XmlSchema::new(vec![XmlField::new("value", ".", Utf8, true)]).unwrap();
    assert!(XmlBatchBuilder::new(schema.clone(), 0).is_err());
    assert!(XmlBatchBuilder::new_projected(schema.clone(), 1, &[1]).is_err());
    assert!(XmlBatchBuilder::new_projected(schema, 1, &[0, 0]).is_err());
}
