use arrow_array::{
    cast::AsArray,
    types::{Float64Type, Int64Type},
    Array, RecordBatch,
};
use arrow_schema::DataType::{Float64, Int64, Utf8};
use futures_util::{io::Cursor, TryStreamExt};
use xml_arrow::{
    InferenceOptions, NamespaceBinding, ReaderOptions, XmlBatchBuilder, XmlField, XmlReader,
    XmlSchema,
};

const CATALOG: &str = include_str!("../testdata/catalog.xml");
const INVENTORY: &str = include_str!("../testdata/inventory.xml");
const CATALOG_NAMES: [&str; 5] = [
    "Notebook & Pen",
    "Desk Lamp",
    "Café Mug",
    "USB Hub",
    "Travel Bag",
];
const DEVICE_IDS: [&str; 8] = [
    "north-gpu-1",
    "north-nic-1",
    "north-gpu-2",
    "north-nic-2",
    "south-gpu-1",
    "south-nic-1",
    "south-gpu-2",
    "south-nic-2",
];
const NAMESPACES: &[NamespaceBinding<'_>] = &[NamespaceBinding {
    prefix: "c",
    uri: "urn:open-arrow-access:catalog",
}];

fn product_fields() -> Vec<XmlField> {
    vec![
        XmlField::new("name", "c:name", Utf8, false),
        XmlField::new("price", "c:price", Float64, false),
        XmlField::new("category", "c:category", Utf8, true),
    ]
}

fn warehouse_fields() -> Vec<XmlField> {
    vec![
        XmlField::new("id", "@id", Int64, false),
        XmlField::list(
            "racks",
            ".",
            false,
            XmlField::structure(
                "item",
                "node",
                false,
                vec![
                    XmlField::new("capacity", "@capacity", Int64, false),
                    XmlField::list(
                        "devices",
                        ".",
                        false,
                        XmlField::structure(
                            "item",
                            "node",
                            false,
                            vec![
                                XmlField::new("id", "@id", Utf8, false),
                                XmlField::new("kind", "@kind", Utf8, false),
                            ],
                        ),
                    ),
                ],
            ),
        ),
    ]
}

async fn batches(xml: &str, schema: XmlSchema, path: &str, batch_size: usize) -> Vec<RecordBatch> {
    let reader = XmlReader::new(
        Cursor::new(xml.as_bytes()),
        schema,
        path,
        ReaderOptions {
            batch_size,
            ..Default::default()
        },
    )
    .unwrap();
    let schema = reader.schema();
    let batches: Vec<_> = reader.into_stream().try_collect().await.unwrap();
    assert!(batches.iter().all(|batch| batch.schema() == schema));
    batches
}

#[test]
fn converts_catalog_with_default_namespace_entities_and_optional_fields() {
    let schema = XmlSchema::with_namespaces(
        vec![
            XmlField::list(
                "products",
                ".",
                false,
                XmlField::structure("item", "c:product", false, product_fields()),
            ),
            XmlField::structure(
                "settings",
                "c:settings",
                false,
                vec![
                    XmlField::new("version", "c:format.version", Int64, false),
                    XmlField::new("label", "c:label", Utf8, false),
                ],
            ),
        ],
        NAMESPACES,
    )
    .unwrap();
    let mut builder = XmlBatchBuilder::new(schema, 1).unwrap();
    builder.append_xml(CATALOG).unwrap();
    let batch = builder.flush().unwrap().unwrap();
    assert_eq!((batch.num_rows(), batch.num_columns()), (1, 2));
    let products = batch.column(0).as_list::<i32>();
    assert_eq!(products.value_offsets(), &[0, 5]);
    let products = products.values().as_struct();
    assert_eq!(
        products
            .column(0)
            .as_string::<i32>()
            .iter()
            .collect::<Vec<_>>(),
        CATALOG_NAMES.map(Some)
    );
    assert_eq!(
        products
            .column(1)
            .as_primitive::<Float64Type>()
            .values()
            .as_ref(),
        &[12.5, 39.95, 8.0, 24.5, 45.0]
    );
    assert_eq!(products.column(2).as_string::<i32>().value(0), "stationery");
    assert_eq!(products.column(2).null_count(), 4);
    let settings = batch.column(1).as_struct();
    assert_eq!(settings.column(0).as_primitive::<Int64Type>().value(0), 2);
    assert_eq!(
        settings.column(1).as_string::<i32>().value(0),
        "${product.name}"
    );
}

#[test]
fn converts_inventory_with_nested_attribute_lists() {
    let schema = XmlSchema::new(vec![
        XmlField::new("version", "@version", Int64, false),
        XmlField::list(
            "warehouses",
            ".",
            false,
            XmlField::structure("item", "warehouse", false, warehouse_fields()),
        ),
    ])
    .unwrap();
    let mut builder = XmlBatchBuilder::new(schema, 1).unwrap();
    builder.append_xml(INVENTORY).unwrap();
    let batch = builder.flush().unwrap().unwrap();
    assert_eq!(batch.num_rows(), 1);
    assert_eq!(batch.column(0).as_primitive::<Int64Type>().value(0), 1);
    let warehouses = batch.column(1).as_list::<i32>();
    assert_eq!(warehouses.value_offsets(), &[0, 2]);
    let warehouses = warehouses.values().as_struct();
    assert_eq!(
        warehouses
            .column(0)
            .as_primitive::<Int64Type>()
            .values()
            .as_ref(),
        &[0, 1]
    );
    let racks = warehouses.column(1).as_list::<i32>();
    assert_eq!(racks.value_offsets(), &[0, 2, 4]);
    let racks = racks.values().as_struct();
    assert_eq!(
        racks
            .column(0)
            .as_primitive::<Int64Type>()
            .values()
            .as_ref(),
        &[16; 4]
    );
    let devices = racks.column(1).as_list::<i32>();
    assert_eq!(devices.value_offsets(), &[0, 2, 4, 6, 8]);
    let devices = devices.values().as_struct();
    assert_eq!(
        devices
            .column(0)
            .as_string::<i32>()
            .iter()
            .collect::<Vec<_>>(),
        DEVICE_IDS.map(Some)
    );
    assert_eq!(
        devices
            .column(1)
            .as_string::<i32>()
            .iter()
            .collect::<Vec<_>>(),
        [
            Some("gpu"),
            Some("network"),
            Some("gpu"),
            Some("network"),
            Some("gpu"),
            Some("network"),
            Some("gpu"),
            Some("network")
        ]
    );
}

#[tokio::test]
async fn streams_catalog_products_with_inherited_namespace() {
    let schema = XmlSchema::with_namespaces(product_fields(), NAMESPACES).unwrap();
    let batches = batches(CATALOG, schema, "/c:catalog/c:product", 2).await;
    assert_eq!(
        batches
            .iter()
            .map(RecordBatch::num_rows)
            .collect::<Vec<_>>(),
        [2, 2, 1]
    );
    assert_eq!(
        batches
            .iter()
            .flat_map(|b| b.column(0).as_string::<i32>().iter())
            .collect::<Vec<_>>(),
        CATALOG_NAMES.map(Some)
    );
    assert_eq!(
        batches
            .iter()
            .map(|b| b.column(2).null_count())
            .sum::<usize>(),
        4
    );
}

#[tokio::test]
async fn streams_inventory_warehouses_as_nested_rows() {
    let schema = XmlSchema::new(warehouse_fields()).unwrap();
    let batches = batches(INVENTORY, schema, "/inventory/warehouse", 1).await;
    assert_eq!(batches.len(), 2);
    for (id, batch) in batches.iter().enumerate() {
        assert_eq!(batch.num_rows(), 1);
        assert_eq!(
            batch.column(0).as_primitive::<Int64Type>().value(0),
            id as i64
        );
        let racks = batch.column(1).as_list::<i32>();
        assert_eq!(racks.value_offsets(), &[0, 2]);
        let devices = racks.values().as_struct().column(1).as_list::<i32>();
        assert_eq!(devices.value_offsets(), &[0, 2, 4]);
        let devices = devices.values().as_struct();
        assert_eq!(
            devices
                .column(0)
                .as_string::<i32>()
                .iter()
                .collect::<Vec<_>>(),
            DEVICE_IDS[id * 4..id * 4 + 4]
                .iter()
                .copied()
                .map(Some)
                .collect::<Vec<_>>()
        );
    }
}

#[tokio::test]
async fn selects_nested_nodes_at_exact_record_depth() {
    let schema = XmlSchema::new(vec![XmlField::new("id", "@id", Utf8, false)]).unwrap();
    let batches = batches(INVENTORY, schema, "/inventory/warehouse/node/node", 3).await;
    assert_eq!(
        batches
            .iter()
            .map(RecordBatch::num_rows)
            .collect::<Vec<_>>(),
        [3, 3, 2]
    );
    assert_eq!(
        batches
            .iter()
            .flat_map(|b| b.column(0).as_string::<i32>().iter())
            .collect::<Vec<_>>(),
        DEVICE_IDS.map(Some)
    );
}

#[tokio::test]
async fn infers_catalog_schema_and_replays_products() {
    let reader = XmlReader::infer(
        Cursor::new(CATALOG.as_bytes()),
        "/c:catalog/c:product",
        NAMESPACES,
        ReaderOptions::default(),
        InferenceOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        reader
            .schema()
            .field_with_name("c:price")
            .unwrap()
            .data_type(),
        &Float64
    );
    let batches: Vec<_> = reader.into_stream().try_collect().await.unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].num_rows(), 5);
    assert_eq!(
        batches[0]
            .column_by_name("c:name")
            .unwrap()
            .as_string::<i32>()
            .iter()
            .collect::<Vec<_>>(),
        CATALOG_NAMES.map(Some)
    );
    assert_eq!(
        batches[0]
            .column_by_name("c:price")
            .unwrap()
            .as_primitive::<Float64Type>()
            .values()
            .as_ref(),
        &[12.5, 39.95, 8.0, 24.5, 45.0]
    );
    assert_eq!(
        batches[0]
            .column_by_name("c:category")
            .unwrap()
            .null_count(),
        4
    );
}
