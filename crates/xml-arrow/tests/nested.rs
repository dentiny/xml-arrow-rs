use arrow_array::{cast::AsArray, types::Int64Type, Array};
use arrow_schema::DataType::{Int64, Utf8};
use xml_arrow::{XmlBatchBuilder, XmlField, XmlSchema};

#[test]
fn structs_preserve_parent_nulls_and_required_children() {
    let schema = XmlSchema::new(vec![XmlField::structure(
        "profile",
        "profile",
        true,
        vec![
            XmlField::new("id", "@id", Int64, false),
            XmlField::structure(
                "address",
                "address",
                true,
                vec![XmlField::new("city", "city", Utf8, false)],
            ),
        ],
    )])
    .unwrap();
    let mut b = XmlBatchBuilder::new(schema, 2).unwrap();
    b.append_xml(r#"<r><profile id="7"><address><city>Paris</city></address></profile></r>"#)
        .unwrap();
    b.append_xml("<r/>").unwrap();
    let batch = b.flush().unwrap().unwrap();
    let profile = batch.column(0).as_struct();
    assert_eq!(profile.column(0).as_primitive::<Int64Type>().value(0), 7);
    assert!(profile.is_null(1));
    let address = profile.column(1).as_struct();
    assert_eq!(address.column(0).as_string::<i32>().value(0), "Paris");
    assert!(address.is_null(1));
    b.append_xml(r#"<r><profile id="8"/></r>"#).unwrap();
    b.append_xml(
        r#"<r xmlns:n="http://www.w3.org/2001/XMLSchema-instance"><profile n:nil="true"/></r>"#,
    )
    .unwrap();
    let batch = b.flush().unwrap().unwrap();
    assert!(batch.column(0).as_struct().column(1).is_null(0));
    assert!(batch.column(0).is_null(1));
}

#[test]
fn lists_distinguish_empty_missing_nil_and_nullable_items() {
    let schema = XmlSchema::new(vec![XmlField::list(
        "tags",
        "tags",
        true,
        XmlField::new("item", "tag", Utf8, true),
    )])
    .unwrap();
    let mut b = XmlBatchBuilder::new(schema, 4).unwrap();
    for xml in [
        r#"<r xmlns:n="http://www.w3.org/2001/XMLSchema-instance"><tags><tag>one</tag><tag n:nil="1"/><tag/></tags></r>"#,
        "<r><tags/></r>",
        "<r/>",
        r#"<r xmlns:n="http://www.w3.org/2001/XMLSchema-instance"><tags n:nil="true"/></r>"#,
    ] {
        b.append_xml(xml).unwrap();
    }
    let batch = b.flush().unwrap().unwrap();
    let tags = batch.column(0).as_list::<i32>();
    assert_eq!(tags.value_offsets(), &[0, 3, 3, 3, 3]);
    assert!(!tags.is_null(1));
    assert!(tags.is_null(2));
    assert!(tags.is_null(3));
    assert_eq!(
        tags.values().as_string::<i32>().iter().collect::<Vec<_>>(),
        vec![Some("one"), None, Some("")]
    );
}

#[test]
fn list_of_structs_keeps_nested_lists_and_item_order() {
    let item = XmlField::structure(
        "item",
        "person",
        false,
        vec![
            XmlField::new("id", "@id", Int64, false),
            XmlField::list(
                "scores",
                "scores",
                false,
                XmlField::new("item", "score", Int64, false),
            ),
        ],
    );
    let schema = XmlSchema::new(vec![XmlField::list("people", "people", false, item)]).unwrap();
    let mut b = XmlBatchBuilder::new(schema, 2).unwrap();
    b.append_xml(r#"<r><people><person id="2"><scores><score>4</score><score>5</score></scores></person><person id="1"><scores/></person></people></r>"#).unwrap();
    let batch = b.flush().unwrap().unwrap();
    let people = batch.column(0).as_list::<i32>();
    let items = people.values().as_struct();
    assert_eq!(
        items
            .column(0)
            .as_primitive::<Int64Type>()
            .values()
            .as_ref(),
        &[2, 1]
    );
    let scores = items.column(1).as_list::<i32>();
    assert_eq!(scores.value_offsets(), &[0, 2, 2]);
    assert_eq!(
        scores
            .values()
            .as_primitive::<Int64Type>()
            .values()
            .as_ref(),
        &[4, 5]
    );
}

#[test]
fn nested_list_offsets_reset_after_flush() {
    let item = XmlField::list(
        "item",
        "row",
        true,
        XmlField::new("item", "v", Int64, false),
    );
    let schema = XmlSchema::new(vec![XmlField::list("matrix", "matrix", false, item)]).unwrap();
    let mut b = XmlBatchBuilder::new(schema, 2).unwrap();
    b.append_xml(r#"<r xmlns:n="http://www.w3.org/2001/XMLSchema-instance"><matrix><row><v>1</v></row><row/><row n:nil="true"/></matrix></r>"#)
        .unwrap();
    let first = b.flush().unwrap().unwrap();
    let matrix = first.column(0).as_list::<i32>();
    assert_eq!(matrix.value_offsets(), &[0, 3]);
    assert_eq!(
        matrix.values().as_list::<i32>().value_offsets(),
        &[0, 1, 1, 1]
    );
    assert!(!matrix.values().is_null(1));
    assert!(matrix.values().is_null(2));
    b.append_xml("<r><matrix><row><v>6</v><v>7</v></row></matrix></r>")
        .unwrap();
    let second = b.flush().unwrap().unwrap();
    let matrix = second.column(0).as_list::<i32>();
    assert_eq!(matrix.value_offsets(), &[0, 1]);
    let row = matrix.values().as_list::<i32>();
    assert_eq!(row.value_offsets(), &[0, 2]);
    assert_eq!(
        row.values().as_primitive::<Int64Type>().values().as_ref(),
        &[6, 7]
    );
}

#[test]
fn namespace_uris_match_across_prefixes_default_namespaces_and_projection() {
    let schema = XmlSchema::with_namespaces(
        vec![
            XmlField::new("unused", "unused", Int64, false),
            XmlField::structure(
                "person",
                "p:person",
                false,
                vec![
                    XmlField::new("qualified_id", "@m:id", Int64, false),
                    XmlField::new("id", "@id", Int64, false),
                    XmlField::new("name", "p:name", Utf8, false),
                    XmlField::new("plain", "name", Utf8, true),
                    XmlField::new("lang", "@xml:lang", Utf8, true),
                ],
            ),
        ],
        &[("p", "urn:person"), ("m", "urn:meta")],
    )
    .unwrap();
    let mut b = XmlBatchBuilder::new_projected(schema, 2, &[1]).unwrap();
    b.append_xml(r#"<r xmlns:a="urn:person" xmlns:b="urn:meta"><a:person b:id="7" id="8" xml:lang="en"><a:name>Jane</a:name></a:person></r>"#).unwrap();
    b.append_xml(r#"<r xmlns="urn:person" xmlns:m="urn:meta"><person m:id="9" id="10"><name>Lee</name><name xmlns="">Plain</name></person></r>"#).unwrap();
    let batch = b.flush().unwrap().unwrap();
    let people = batch.column(0).as_struct();
    assert_eq!(
        people
            .column(0)
            .as_primitive::<Int64Type>()
            .values()
            .as_ref(),
        &[7, 9]
    );
    assert_eq!(
        people
            .column(1)
            .as_primitive::<Int64Type>()
            .values()
            .as_ref(),
        &[8, 10]
    );
    assert_eq!(
        people
            .column(2)
            .as_string::<i32>()
            .iter()
            .collect::<Vec<_>>(),
        vec![Some("Jane"), Some("Lee")]
    );
    assert_eq!(
        people
            .column(3)
            .as_string::<i32>()
            .iter()
            .collect::<Vec<_>>(),
        vec![None, Some("Plain")]
    );
    assert_eq!(
        people
            .column(4)
            .as_string::<i32>()
            .iter()
            .collect::<Vec<_>>(),
        vec![Some("en"), None]
    );
}

#[test]
fn invalid_nested_values_leave_the_batch_reusable() {
    let schema = XmlSchema::new(vec![
        XmlField::structure(
            "profile",
            "profile",
            false,
            vec![XmlField::new("id", "@id", Int64, false)],
        ),
        XmlField::list(
            "tags",
            "tags",
            false,
            XmlField::new("item", "tag", Int64, false),
        ),
    ])
    .unwrap();
    let mut b = XmlBatchBuilder::new(schema, 2).unwrap();
    b.append_xml(r#"<r><profile id="1"/><tags><tag>2</tag></tags></r>"#)
        .unwrap();
    for invalid in [
        "<profile/><tags/>",
        r#"<profile id="2"/><tags><tag>oops</tag></tags>"#,
        r#"<profile id="2"/><tags><tag n:nil="true"/></tags>"#,
        r#"<profile n:nil="true"/><tags/>"#,
        r#"<profile id="2" n:nil="maybe"/><tags/>"#,
        r#"<profile id="2"/><tags n:nil="true"><tag>2</tag></tags>"#,
        r#"<profile id="2">text<extra/></profile><tags/>"#,
        r#"<profile id="2"/><tags/><tags/>"#,
    ] {
        let xml =
            format!(r#"<r xmlns:n="http://www.w3.org/2001/XMLSchema-instance">{invalid}</r>"#);
        assert!(b.append_xml(&xml).is_err(), "{invalid}");
        assert_eq!(b.len(), 1);
    }
    b.append_xml(r#"<r xmlns:n="http://www.w3.org/2001/XMLSchema-instance"><profile id="3" n:nil="false"/><tags/></r>"#).unwrap();
    let batch = b.flush().unwrap().unwrap();
    assert_eq!(batch.num_rows(), 2);
    assert_eq!(
        batch
            .column(0)
            .as_struct()
            .column(0)
            .as_primitive::<Int64Type>()
            .values()
            .as_ref(),
        &[1, 3]
    );
    assert_eq!(batch.column(1).as_list::<i32>().value_offsets(), &[0, 1, 1]);
}
