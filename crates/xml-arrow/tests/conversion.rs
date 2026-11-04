use arrow_array::{Array, BooleanArray, Float64Array, Int64Array, StringArray};
use arrow_schema::DataType;
use xml_arrow::{Error, XmlBatchBuilder, XmlField, XmlSchema};

fn field(name: &str, path: &str, data_type: DataType, nullable: bool) -> XmlField {
    XmlField::new(name, path, data_type, nullable)
}

fn builder(fields: Vec<XmlField>) -> XmlBatchBuilder {
    XmlBatchBuilder::new(XmlSchema::new(fields).unwrap(), 2).unwrap()
}

#[test]
fn converts_all_types_and_descendant_attribute() {
    let mut b = builder(vec![
        field("id", "@id", DataType::Int64, false),
        field("name", "name", DataType::Utf8, false),
        field("active", "active", DataType::Boolean, false),
        field("price", "price", DataType::Float64, false),
        field("author", "author/@id", DataType::Utf8, false),
    ]);
    b.append_xml(r#"<record id="42"><name> hello </name><active> 1 </active><price> -1.25e2 </price><author id="a"/></record>"#).unwrap();
    b.append_xml(r#"<record id="-3"><name>world</name><active>false</active><price>0.5</price><author id="b"/></record>"#).unwrap();
    let batch = b.flush().unwrap().unwrap();
    assert_eq!(batch.num_rows(), 2);
    assert_eq!(
        batch
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .values()
            .as_ref(),
        &[42, -3]
    );
    assert_eq!(
        batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        " hello "
    );
    assert!(batch
        .column(2)
        .as_any()
        .downcast_ref::<BooleanArray>()
        .unwrap()
        .value(0));
    assert!(!batch
        .column(2)
        .as_any()
        .downcast_ref::<BooleanArray>()
        .unwrap()
        .value(1));
    assert_eq!(
        batch
            .column(3)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap()
            .value(0),
        -125.0
    );
    assert_eq!(
        batch
            .column(4)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(1),
        "b"
    );
}

#[test]
fn missing_is_null_but_empty_is_present() {
    let mut b = builder(vec![
        field("text", "text", DataType::Utf8, true),
        field("attr", "@attr", DataType::Utf8, true),
    ]);
    b.append_xml("<r/>").unwrap();
    b.append_xml(r#"<r attr=""><text/></r>"#).unwrap();
    let batch = b.flush().unwrap().unwrap();
    for column in batch.columns() {
        let strings = column.as_any().downcast_ref::<StringArray>().unwrap();
        assert!(strings.is_null(0));
        assert!(!strings.is_null(1));
        assert_eq!(strings.value(1), "");
    }
}

#[test]
fn required_missing_error_includes_column_and_path() {
    let mut b = builder(vec![field("identifier", "@id", DataType::Int64, false)]);
    let err = b.append_xml("<r/>").unwrap_err();
    assert!(
        matches!(&err, Error::Field { name, path, .. } if name == "identifier" && path == "@id")
    );
    assert!(b.is_empty());
}

#[test]
fn references_cdata_comments_and_unicode_preserve_text() {
    let mut b = builder(vec![
        field("text", ".", DataType::Utf8, false),
        field("attr", "@v", DataType::Utf8, false),
    ]);
    b.append_xml("<r v=\"a&amp;b&#x1F600;\"> 中&amp;<![CDATA[<raw>&amp;]]><!-- comment -->&#65;&#x1F600; </r>").unwrap();
    let batch = b.flush().unwrap().unwrap();
    assert_eq!(
        batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        " 中&<raw>&amp;A😀 "
    );
    assert_eq!(
        batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "a&b😀"
    );
}

#[test]
fn xml_line_endings_and_attribute_whitespace_are_normalized() {
    let mut b = builder(vec![
        field("text", ".", DataType::Utf8, false),
        field("attr", "@v", DataType::Utf8, false),
    ]);
    b.append_xml("<r v=\"a\tb\nc\r\nd&#10;e\">a\r\nb\rc<![CDATA[d\r\ne]]></r>")
        .unwrap();
    let batch = b.flush().unwrap().unwrap();
    assert_eq!(
        batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "a\nb\ncd\ne"
    );
    assert_eq!(
        batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "a b c d\ne"
    );
}

#[test]
fn failure_is_atomic_even_after_some_fields_would_convert() {
    let mut b = builder(vec![
        field("a", "a", DataType::Utf8, false),
        field("b", "b", DataType::Int64, false),
    ]);
    b.append_xml("<r><a>first</a><b>1</b></r>").unwrap();
    assert!(b.append_xml("<r><a>bad</a><b>oops</b></r>").is_err());
    assert_eq!(b.len(), 1);
    b.append_xml("<r><a>second</a><b>2</b></r>").unwrap();
    let batch = b.flush().unwrap().unwrap();
    assert_eq!(batch.num_rows(), 2);
    assert_eq!(
        batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(1),
        "second"
    );
    assert_eq!(
        batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(1),
        2
    );
}

#[test]
fn capacity_and_flush_allow_reuse() {
    let mut b = builder(vec![field("text", ".", DataType::Utf8, false)]);
    assert!(b.flush().unwrap().is_none());
    b.append_xml("<r>1</r>").unwrap();
    b.append_xml("<r>2</r>").unwrap();
    assert!(b.is_full());
    assert!(matches!(b.append_xml("<r>3</r>"), Err(Error::BatchFull)));
    assert_eq!(b.flush().unwrap().unwrap().num_rows(), 2);
    assert!(b.is_empty());
    b.append_xml("<r>3</r>").unwrap();
    let batch = b.flush().unwrap().unwrap();
    assert_eq!(batch.num_rows(), 1);
    assert_eq!(
        batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "3"
    );
    assert!(b.flush().unwrap().is_none());
}

#[test]
fn projection_reorders_and_does_not_convert_excluded_fields() {
    let schema = XmlSchema::new(vec![
        field("id", "@id", DataType::Int64, false),
        field("unused", "unused", DataType::Boolean, false),
        field("name", "name", DataType::Utf8, false),
    ])
    .unwrap();
    let mut b = XmlBatchBuilder::new_projected(schema, 2, &[2, 0]).unwrap();
    b.append_xml(r#"<r id="5"><name>five</name><unused>invalid</unused><unused/></r>"#)
        .unwrap();
    b.append_xml(r#"<r id="6"><name>six</name></r>"#).unwrap();
    assert_eq!(b.schema().field(0).name(), "name");
    let batch = b.flush().unwrap().unwrap();
    assert_eq!(batch.schema().field(1).name(), "id");
    assert_eq!(
        batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(1),
        "six"
    );
    assert_eq!(
        batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(0),
        5
    );
}

#[test]
fn zero_column_projection_counts_rows_and_still_checks_xml() {
    let schema = XmlSchema::new(vec![field("missing", "missing", DataType::Int64, false)]).unwrap();
    let mut b = XmlBatchBuilder::new_projected(schema, 2, &[]).unwrap();
    b.append_xml("<r/>").unwrap();
    assert!(b.append_xml("<r>").is_err());
    b.append_xml("<r><ignored/></r>").unwrap();
    let batch = b.flush().unwrap().unwrap();
    assert_eq!(batch.num_rows(), 2);
    assert_eq!(batch.num_columns(), 0);
}

#[test]
fn empty_schema_can_count_rows() {
    let mut b = builder(vec![]);
    b.append_xml("<r/>").unwrap();
    assert_eq!(b.flush().unwrap().unwrap().num_rows(), 1);
}

#[test]
fn duplicate_path_can_feed_different_output_columns() {
    let mut b = builder(vec![
        field("text", "v", DataType::Utf8, false),
        field("number", "v", DataType::Int64, false),
    ]);
    b.append_xml("<r><v>12</v></r>").unwrap();
    let batch = b.flush().unwrap().unwrap();
    assert_eq!(
        batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "12"
    );
    assert_eq!(
        batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(0),
        12
    );
}

#[test]
fn descendant_paths_and_unknown_fields() {
    let mut b = builder(vec![field("name", "author/name", DataType::Utf8, false)]);
    b.append_xml("<book><ignored><nested>not output</nested></ignored><author><name>Jane</name></author></book>").unwrap();
    assert_eq!(
        b.flush()
            .unwrap()
            .unwrap()
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "Jane"
    );
}

#[test]
fn repeated_scalars_and_nested_text_are_errors() {
    for xml in [
        "<r><v>1</v><v>2</v></r>",
        "<r><v/><v/></r>",
        "<r><v>before<x/>after</v></r>",
    ] {
        let mut b = builder(vec![field("value", "v", DataType::Utf8, true)]);
        assert!(
            matches!(b.append_xml(xml), Err(Error::Field { .. })),
            "{xml}"
        );
        assert!(b.is_empty());
    }
}

#[test]
fn repeated_descendant_attributes_are_errors() {
    let mut b = builder(vec![field("id", "a/@id", DataType::Utf8, true)]);
    assert!(b.append_xml(r#"<r><a id="1"/><a id="2"/></r>"#).is_err());
}

#[test]
fn invalid_scalar_values_do_not_become_null() {
    for (data_type, bad_values) in [
        (
            DataType::Int64,
            vec!["", "1.5", "9223372036854775808", "one"],
        ),
        (DataType::Boolean, vec!["", "TRUE", "yes", "2"]),
        (DataType::Float64, vec!["", "NaN", "inf", "1e9999", "x"]),
    ] {
        for value in bad_values {
            let mut b = builder(vec![field("value", "v", data_type.clone(), true)]);
            assert!(
                matches!(
                    b.append_xml(&format!("<r><v>{value}</v></r>")),
                    Err(Error::Field { .. })
                ),
                "{data_type}: {value}"
            );
        }
    }
}

#[test]
fn xml_failure_after_valid_values_does_not_append_row() {
    let mut b = builder(vec![field("value", "v", DataType::Utf8, false)]);
    b.append_xml("<r><v>ok</v></r>").unwrap();
    assert!(b.append_xml("<r><v>bad</v></r><extra/>").is_err());
    assert_eq!(b.flush().unwrap().unwrap().num_rows(), 1);
}

#[test]
fn unsupported_or_broken_xml_is_rejected_even_without_columns() {
    for input in [
        "",
        " ",
        "<r>",
        "<r><v></r>",
        "<r/><s/>",
        "before<r/>",
        "<r/>after",
        "<r a='1' a='2'/>",
        "<r>&unknown;</r>",
        "<r>&amp</r>",
        "<r>&#0;</r>",
        "<r>\u{1}</r>",
        "<!DOCTYPE r><r/>",
        "<r xmlns='urn:x'/>",
        "<x:r/>",
        "<r x:a='1'/>",
        "<r><!-- bad -- comment --></r>",
        "<![CDATA[ ]]><r/>",
        "<1invalid/>",
        "<r 1invalid='a'/>",
        "<r/>\u{a0}",
        "<r><?xml version='1.0'?></r>",
    ] {
        let mut b = builder(vec![]);
        assert!(
            b.append_xml(input).is_err(),
            "unexpectedly accepted {input:?}"
        );
        assert!(b.is_empty());
    }
}

#[test]
fn accepts_declaration_comments_processing_instructions_and_unicode_names() {
    let mut b = builder(vec![field("名字", "名字", DataType::Utf8, false)]);
    b.append_xml("<?xml version='1.0' encoding='UTF-8'?>\n<!-- before --><?app ok?><记录><名字>好</名字></记录><!-- after -->").unwrap();
    assert_eq!(
        b.flush()
            .unwrap()
            .unwrap()
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "好"
    );
}

#[test]
fn utf8_bom_is_accepted_with_and_without_declaration() {
    for xml in ["\u{FEFF}<r/>", "\u{FEFF}<?xml version='1.0'?><r/>"] {
        let mut b = builder(vec![]);
        b.append_xml(xml).unwrap();
        assert_eq!(b.len(), 1);
    }
}

#[test]
fn declaration_metadata_does_not_redecode_rust_strings() {
    for declaration in [
        "<?xml version='1.1'?>",
        "<?xml version='1.0' encoding='UTF-16'?>",
    ] {
        let mut b = builder(vec![field("text", ".", DataType::Utf8, false)]);
        b.append_xml(&format!("{declaration}<r>已经解码</r>"))
            .unwrap();
        let batch = b.flush().unwrap().unwrap();
        assert_eq!(
            batch
                .column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap()
                .value(0),
            "已经解码"
        );
    }
}

#[test]
fn descendant_paths_search_all_container_branches() {
    let mut b = builder(vec![
        field("value", "a/b", DataType::Utf8, true),
        field("id", "a/@id", DataType::Int64, true),
    ]);
    b.append_xml(r#"<r><a/><a id="7"><b>found</b></a></r>"#)
        .unwrap();
    assert!(b
        .append_xml(r#"<r><a><b>first</b></a><a><b>second</b></a></r>"#)
        .is_err());
    let batch = b.flush().unwrap().unwrap();
    assert_eq!(batch.num_rows(), 1);
    assert_eq!(
        batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "found"
    );
    assert_eq!(
        batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(0),
        7
    );
}

#[test]
fn syntax_errors_retain_parser_location_and_source() {
    let mut b = builder(vec![]);
    let error = b.append_xml("<r>\n<value></r>").unwrap_err();
    assert!(std::error::Error::source(&error).is_some());
    match error {
        Error::Xml(source) => assert_eq!(source.pos().row, 2),
        other => panic!("expected a parser error, got {other}"),
    }
}
