use arrow_schema::DataType::{Binary, Int64, Utf8};
use xml_arrow::{NamespaceBinding, XmlBatchBuilder, XmlField, XmlSchema};

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

#[test]
fn rejects_invalid_nested_mappings_and_namespace_bindings() {
    for field in [
        XmlField::list(
            "values",
            "@values",
            true,
            XmlField::new("item", "v", Utf8, false),
        ),
        XmlField::structure(
            "record",
            ".",
            false,
            vec![
                XmlField::new("id", "@id", Int64, false),
                XmlField::new("id", "id", Int64, true),
            ],
        ),
        XmlField::new("value", "unknown:value", Utf8, true),
        XmlField::new(
            "values",
            ".",
            arrow_schema::DataType::List(std::sync::Arc::new(arrow_schema::Field::new(
                "item", Utf8, true,
            ))),
            true,
        ),
    ] {
        assert!(XmlSchema::new(vec![field]).is_err());
    }
    for bindings in [
        vec![
            NamespaceBinding {
                prefix: "p",
                uri: "urn:a",
            },
            NamespaceBinding {
                prefix: "p",
                uri: "urn:b",
            },
        ],
        vec![NamespaceBinding {
            prefix: "xml",
            uri: "urn:wrong",
        }],
        vec![NamespaceBinding {
            prefix: "p",
            uri: "",
        }],
    ] {
        assert!(XmlSchema::with_namespaces(vec![], &bindings).is_err());
    }
}
