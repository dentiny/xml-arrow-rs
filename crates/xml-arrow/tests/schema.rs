use arrow_schema::DataType;
use xml_arrow::{XmlBatchBuilder, XmlField, XmlSchema};

#[test]
fn invalid_paths_are_rejected_at_configuration_time() {
    for path in [
        "", "/a", "a/", "a//b", "a/*", "a[1]", "a/..", "a/.", "@id/a", "a/@", "a/@x/b", "a:b",
        "a b", "1a", "@@id",
    ] {
        assert!(
            XmlSchema::new(vec![XmlField::new("value", path, DataType::Utf8, true)]).is_err(),
            "{path}"
        );
    }
}

#[test]
fn valid_paths_and_arrow_fields_are_retained() {
    for path in [".", "@id", "title", "a/b", "a/@id", "名字", "_a/b-1"] {
        let schema =
            XmlSchema::new(vec![XmlField::new("value", path, DataType::Utf8, false)]).unwrap();
        assert_eq!(schema.fields()[0].path(), path);
        assert_eq!(
            schema.fields()[0].arrow_field(),
            schema.arrow_schema().field(0)
        );
        assert!(!schema.arrow_schema().field(0).is_nullable());
    }
}

#[test]
fn invalid_names_types_and_duplicate_columns_fail() {
    assert!(XmlSchema::new(vec![XmlField::new("", ".", DataType::Utf8, false)]).is_err());
    assert!(XmlSchema::new(vec![
        XmlField::new("x", ".", DataType::Utf8, true),
        XmlField::new("x", "@id", DataType::Int64, false)
    ])
    .is_err());
    for data_type in [
        DataType::Int32,
        DataType::Null,
        DataType::Binary,
        DataType::Date32,
    ] {
        assert!(XmlSchema::new(vec![XmlField::new("x", ".", data_type, true)]).is_err());
    }
}

#[test]
fn invalid_projection_and_zero_batch_size_fail() {
    let schema = XmlSchema::new(vec![XmlField::new("x", ".", DataType::Utf8, true)]).unwrap();
    assert!(XmlBatchBuilder::new(schema.clone(), 0).is_err());
    assert!(XmlBatchBuilder::new_projected(schema.clone(), 1, &[1]).is_err());
    assert!(XmlBatchBuilder::new_projected(schema, 1, &[0, 0]).is_err());
}
