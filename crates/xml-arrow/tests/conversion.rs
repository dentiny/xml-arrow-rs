use arrow_array::{
    cast::AsArray,
    types::{Float64Type, Int64Type},
    Array,
};
use arrow_schema::DataType::{Boolean, Float64, Int64, Utf8};
use xml_arrow::{Error, XmlBatchBuilder, XmlField, XmlSchema};

fn builder(fields: Vec<XmlField>) -> XmlBatchBuilder {
    XmlBatchBuilder::new(XmlSchema::new(fields).unwrap(), 2).unwrap()
}

#[test]
fn converts_scalar_fields() {
    let mut b = builder(vec![
        XmlField::new("id", "@id", Int64, false),
        XmlField::new("title", "details/title", Utf8, false),
        XmlField::new("active", "active", Boolean, false),
        XmlField::new("price", "price", Float64, false),
        XmlField::new("author", "author/@id", Utf8, false),
    ]);
    b.append_xml(
        r#"<book id="42">
        <details><title>Rust &amp; <![CDATA[<Arrow>]]></title></details>
        <active>1</active><price>29.5</price><author id="a"/>
    </book>"#,
    )
    .unwrap();
    let batch = b.flush().unwrap().unwrap();
    assert_eq!((batch.num_rows(), batch.num_columns()), (1, 5));
    assert_eq!(batch.column(0).as_primitive::<Int64Type>().value(0), 42);
    assert_eq!(
        batch.column(1).as_string::<i32>().value(0),
        "Rust & <Arrow>"
    );
    assert!(batch.column(2).as_boolean().value(0));
    assert_eq!(batch.column(3).as_primitive::<Float64Type>().value(0), 29.5);
    assert_eq!(batch.column(4).as_string::<i32>().value(0), "a");
}

#[test]
fn distinguishes_missing_empty_and_required_values() {
    let mut b = builder(vec![
        XmlField::new("id", "@id", Int64, false),
        XmlField::new("text", "text", Utf8, true),
    ]);
    assert!(matches!(b.append_xml("<r/>"), Err(Error::Field { .. })));
    b.append_xml(r#"<r id="1"/>"#).unwrap();
    b.append_xml(r#"<r id="2"><text/></r>"#).unwrap();
    let batch = b.flush().unwrap().unwrap();
    let text = batch.column(1).as_string::<i32>();
    assert_eq!(batch.num_rows(), 2);
    assert!(text.is_null(0));
    assert!(!text.is_null(1));
    assert_eq!(text.value(1), "");
}

#[test]
fn failed_append_does_not_change_batch() {
    let mut b = builder(vec![
        XmlField::new("text", "text", Utf8, false),
        XmlField::new("id", "id", Int64, true),
    ]);
    b.append_xml("<r><text>first</text><id>1</id></r>").unwrap();
    for invalid in [
        "<r><text>bad</text><id>oops</id></r>",
        "<r><text>bad</text><id>2</id><id>3</id></r>",
        "<r><text>bad<child/></text><id>2</id></r>",
    ] {
        assert!(matches!(b.append_xml(invalid), Err(Error::Field { .. })));
        assert_eq!(b.len(), 1);
    }
    b.append_xml("<r><text>second</text><id>2</id></r>")
        .unwrap();
    let batch = b.flush().unwrap().unwrap();
    assert_eq!(batch.num_rows(), 2);
    assert_eq!(batch.column(0).as_string::<i32>().value(1), "second");
    assert_eq!(
        batch
            .column(1)
            .as_primitive::<Int64Type>()
            .values()
            .as_ref(),
        &[1, 2]
    );
}

#[test]
fn flush_respects_capacity_and_allows_reuse() {
    let mut b = builder(vec![XmlField::new("text", ".", Utf8, false)]);
    assert!(b.flush().unwrap().is_none());
    b.append_xml("<r>1</r>").unwrap();
    b.append_xml("<r>2</r>").unwrap();
    assert!(b.is_full());
    assert!(matches!(b.append_xml("<r>3</r>"), Err(Error::BatchFull)));
    assert_eq!(b.flush().unwrap().unwrap().num_rows(), 2);
    b.append_xml("<r>3</r>").unwrap();
    let batch = b.flush().unwrap().unwrap();
    assert_eq!(batch.num_rows(), 1);
    assert_eq!(batch.column(0).as_string::<i32>().value(0), "3");
    assert!(b.flush().unwrap().is_none());
}

#[test]
fn projection_reorders_and_skips_excluded_values() {
    let schema = XmlSchema::new(vec![
        XmlField::new("id", "@id", Int64, false),
        XmlField::new("unused", "unused", Boolean, false),
        XmlField::new("name", "name", Utf8, false),
    ])
    .unwrap();
    let mut b = XmlBatchBuilder::new_projected(schema, 2, &[2, 0]).unwrap();
    b.append_xml(r#"<r id="5"><name>five</name><unused>invalid</unused></r>"#)
        .unwrap();
    let batch = b.flush().unwrap().unwrap();
    assert_eq!(batch.schema().field(0).name(), "name");
    assert_eq!(batch.schema().field(1).name(), "id");
    assert_eq!(batch.column(0).as_string::<i32>().value(0), "five");
    assert_eq!(batch.column(1).as_primitive::<Int64Type>().value(0), 5);
}

#[test]
fn zero_column_projection_preserves_rows_and_checks_xml() {
    let schema = XmlSchema::new(vec![XmlField::new("missing", "missing", Int64, false)]).unwrap();
    let mut b = XmlBatchBuilder::new_projected(schema, 2, &[]).unwrap();
    b.append_xml("<r/>").unwrap();
    assert!(matches!(b.append_xml("<r>"), Err(Error::Xml(_))));
    b.append_xml("<r><ignored/></r>").unwrap();
    let batch = b.flush().unwrap().unwrap();
    assert_eq!((batch.num_rows(), batch.num_columns()), (2, 0));
}

#[test]
fn direct_namespaced_fields_match_nested_paths_and_reject_duplicates() {
    use arrow_schema::DataType::{Int64, Utf8};
    use xml_arrow::{NamespaceBinding, XmlBatchBuilder, XmlField, XmlSchema};
    let bindings = [NamespaceBinding {
        prefix: "m",
        uri: "urn:test",
    }];
    let schema = XmlSchema::with_namespaces(
        vec![
            XmlField::new("id", "@m:id", Int64, false),
            XmlField::new("text", "m:text", Utf8, false),
            XmlField::new("missing", "missing", Utf8, true),
        ],
        &bindings,
    )
    .unwrap();
    let mut builder = XmlBatchBuilder::new(schema, 4).unwrap();
    builder
        .append_xml("<row xmlns:p='urn:test' p:id='42'><p:text>a<![CDATA[b]]>c</p:text></row>")
        .unwrap();
    let good = builder.len();
    for invalid in [
        "<row xmlns:p='urn:test' p:id='42'><p:text>a</p:text><p:text>b</p:text></row>",
        "<row xmlns:p='urn:test' id='42'><p:text>a</p:text></row>",
        "<row xmlns:p='urn:test' p:id='42'><text>a</text></row>",
    ] {
        assert!(builder.append_xml(invalid).is_err());
        assert_eq!(builder.len(), good);
    }
    let batch = builder.flush().unwrap().unwrap();
    let id = batch
        .column(0)
        .as_any()
        .downcast_ref::<arrow_array::Int64Array>()
        .unwrap();
    let text = batch
        .column(1)
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .unwrap();
    assert_eq!(id.value(0), 42);
    assert_eq!(text.value(0), "abc");
    assert!(batch.column(2).is_null(0));
}
