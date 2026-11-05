use arrow_array::{cast::AsArray, types::Int64Type, Array};
use arrow_schema::DataType::{Int64, Utf8};
use xml_arrow::{XmlBatchBuilder, XmlField, XmlSchema};

const POM: &str = include_str!("../../../testdata/lancedb-pom.xml");
const TOPOLOGY: &str = include_str!("../../../testdata/azure-ndv5-topology.xml");

#[test]
fn converts_real_maven_pom_with_default_namespace_and_dependencies() {
    let dependency = XmlField::structure(
        "item",
        "m:dependency",
        false,
        vec![
            XmlField::new("group", "m:groupId", Utf8, false),
            XmlField::new("artifact", "m:artifactId", Utf8, false),
            XmlField::new("version", "m:version", Utf8, false),
            XmlField::new("scope", "m:scope", Utf8, true),
        ],
    );
    let schema = XmlSchema::with_namespaces(
        vec![
            XmlField::new("artifact", "m:artifactId", Utf8, false),
            XmlField::structure(
                "properties",
                "m:properties",
                false,
                vec![XmlField::new(
                    "arrow_version",
                    "m:arrow.version",
                    Utf8,
                    false,
                )],
            ),
            XmlField::list(
                "dependencies",
                "m:dependencyManagement/m:dependencies",
                false,
                dependency,
            ),
            XmlField::list(
                "modules",
                "m:modules",
                false,
                XmlField::new("item", "m:module", Utf8, false),
            ),
        ],
        &[("m", "http://maven.apache.org/POM/4.0.0")],
    )
    .unwrap();
    let mut builder = XmlBatchBuilder::new(schema, 1).unwrap();
    builder.append_xml(POM).unwrap();
    let batch = builder.flush().unwrap().unwrap();
    assert_eq!((batch.num_rows(), batch.num_columns()), (1, 4));
    assert_eq!(
        batch.column(0).as_string::<i32>().value(0),
        "lancedb-parent"
    );
    assert_eq!(
        batch
            .column(1)
            .as_struct()
            .column(0)
            .as_string::<i32>()
            .value(0),
        "15.0.0"
    );
    let dependencies = batch.column(2).as_list::<i32>();
    assert_eq!(dependencies.value_offsets(), &[0, 7]);
    let items = dependencies.values().as_struct();
    assert_eq!(
        items
            .column(1)
            .as_string::<i32>()
            .iter()
            .collect::<Vec<_>>(),
        vec![
            Some("jackson-bom"),
            Some("lance-core"),
            Some("arrow-vector"),
            Some("arrow-memory-netty"),
            Some("arrow-c-data"),
            Some("arrow-dataset"),
            Some("junit-jupiter"),
        ]
    );
    assert_eq!(
        items.column(2).as_string::<i32>().value(0),
        "${jackson.version}"
    );
    assert_eq!(
        items.column(2).as_string::<i32>().value(2),
        "${arrow.version}"
    );
    assert_eq!(items.column(3).as_string::<i32>().value(0), "import");
    assert_eq!(items.column(3).null_count(), 6);
    assert_eq!(
        batch
            .column(3)
            .as_list::<i32>()
            .values()
            .as_string::<i32>()
            .value(0),
        "lancedb-core"
    );
}

#[test]
fn converts_real_ndv5_topology_with_nested_attribute_lists() {
    let devices = XmlField::list(
        "devices",
        ".",
        false,
        XmlField::structure(
            "item",
            "pci",
            false,
            vec![
                XmlField::new("bus", "@busid", Utf8, false),
                XmlField::new("class", "@class", Utf8, false),
            ],
        ),
    );
    let bridges = XmlField::list(
        "bridges",
        ".",
        false,
        XmlField::structure(
            "item",
            "pci",
            false,
            vec![
                XmlField::new("bus", "@busid", Utf8, false),
                XmlField::new("width", "@link_width", Int64, false),
                devices,
            ],
        ),
    );
    let cpus = XmlField::list(
        "cpus",
        ".",
        false,
        XmlField::structure(
            "item",
            "cpu",
            false,
            vec![XmlField::new("numa", "@numaid", Int64, false), bridges],
        ),
    );
    let schema = XmlSchema::new(vec![
        XmlField::new("version", "@version", Int64, false),
        cpus,
    ])
    .unwrap();
    let mut builder = XmlBatchBuilder::new(schema, 1).unwrap();
    builder.append_xml(TOPOLOGY).unwrap();
    let batch = builder.flush().unwrap().unwrap();
    assert_eq!(batch.num_rows(), 1);
    assert_eq!(batch.column(0).as_primitive::<Int64Type>().value(0), 1);
    let cpus = batch.column(1).as_list::<i32>();
    assert_eq!(cpus.value_offsets(), &[0, 2]);
    let cpus = cpus.values().as_struct();
    assert_eq!(
        cpus.column(0).as_primitive::<Int64Type>().values().as_ref(),
        &[0, 1]
    );
    let bridges = cpus.column(1).as_list::<i32>();
    assert_eq!(bridges.value_offsets(), &[0, 4, 8]);
    let bridges = bridges.values().as_struct();
    assert_eq!(
        bridges
            .column(1)
            .as_primitive::<Int64Type>()
            .values()
            .as_ref(),
        &[16; 8]
    );
    let devices = bridges.column(2).as_list::<i32>();
    assert_eq!(devices.value_offsets(), &[0, 2, 4, 6, 8, 10, 12, 14, 16]);
    let devices = devices.values().as_struct();
    assert_eq!(
        devices.column(0).as_string::<i32>().value(0),
        "0001:00:00.0"
    );
    assert_eq!(
        devices.column(0).as_string::<i32>().value(15),
        "0108:00:00.0"
    );
    assert_eq!(devices.column(1).as_string::<i32>().value(0), "0x030200");
    assert_eq!(devices.column(1).as_string::<i32>().value(1), "0x020700");
}

mod streaming {
    use arrow_array::{cast::AsArray, types::Int64Type, Array};
    use arrow_schema::DataType::{Int64, Utf8};
    use futures_executor::block_on;
    use futures_util::{
        io::{BufReader, Cursor},
        TryStreamExt,
    };
    use xml_arrow::{ReaderOptions, XmlReader};
    use xml_arrow::{XmlField, XmlSchema};

    #[test]
    fn streams_real_pom_dependencies_with_inherited_default_namespace() {
        block_on(async {
            let schema = XmlSchema::with_namespaces(
                vec![
                    XmlField::new("artifact", "m:artifactId", Utf8, false),
                    XmlField::new("version", "m:version", Utf8, false),
                    XmlField::new("scope", "m:scope", Utf8, true),
                ],
                &[("m", "http://maven.apache.org/POM/4.0.0")],
            )
            .unwrap();
            let input = BufReader::with_capacity(
                7,
                Cursor::new(include_bytes!("../../../testdata/lancedb-pom.xml").as_slice()),
            );
            let reader = XmlReader::new(
                input,
                schema,
                "/m:project/m:dependencyManagement/m:dependencies/m:dependency",
                ReaderOptions {
                    batch_size: 3,
                    ..Default::default()
                },
            )
            .unwrap();
            let batches: Vec<_> = reader.into_stream().try_collect().await.unwrap();
            assert_eq!(
                batches.iter().map(|b| b.num_rows()).collect::<Vec<_>>(),
                vec![3, 3, 1]
            );
            let artifacts = batches
                .iter()
                .flat_map(|b| b.column(0).as_string::<i32>().iter())
                .collect::<Vec<_>>();
            assert_eq!(
                artifacts,
                vec![
                    Some("jackson-bom"),
                    Some("lance-core"),
                    Some("arrow-vector"),
                    Some("arrow-memory-netty"),
                    Some("arrow-c-data"),
                    Some("arrow-dataset"),
                    Some("junit-jupiter")
                ]
            );
            assert_eq!(
                batches[0].column(1).as_string::<i32>().value(0),
                "${jackson.version}"
            );
            assert_eq!(
                batches
                    .iter()
                    .map(|b| b.column(2).null_count())
                    .sum::<usize>(),
                6
            );
        });
    }

    #[test]
    fn streams_real_topology_cpus_as_nested_arrow_rows() {
        block_on(async {
            let devices = XmlField::list(
                "devices",
                ".",
                false,
                XmlField::structure(
                    "item",
                    "pci",
                    false,
                    vec![XmlField::new("bus", "@busid", Utf8, false)],
                ),
            );
            let bridges = XmlField::list(
                "bridges",
                ".",
                false,
                XmlField::structure(
                    "item",
                    "pci",
                    false,
                    vec![XmlField::new("bus", "@busid", Utf8, false), devices],
                ),
            );
            let schema = XmlSchema::new(vec![
                XmlField::new("numa", "@numaid", Int64, false),
                bridges,
            ])
            .unwrap();
            let input = BufReader::with_capacity(
                7,
                Cursor::new(include_bytes!("../../../testdata/azure-ndv5-topology.xml").as_slice()),
            );
            let reader = XmlReader::new(
                input,
                schema,
                "/system/cpu",
                ReaderOptions {
                    batch_size: 1,
                    ..Default::default()
                },
            )
            .unwrap();
            let batches: Vec<_> = reader.into_stream().try_collect().await.unwrap();
            assert_eq!(batches.len(), 2);
            let mut device_count = 0;
            for (numa, batch) in batches.iter().enumerate() {
                assert_eq!(
                    batch.column(0).as_primitive::<Int64Type>().value(0),
                    numa as i64
                );
                let bridges = batch.column(1).as_list::<i32>();
                assert_eq!(bridges.value_offsets(), &[0, 4]);
                let devices = bridges.values().as_struct().column(1).as_list::<i32>();
                assert_eq!(devices.value_offsets(), &[0, 2, 4, 6, 8]);
                device_count += devices.values().len();
            }
            assert_eq!(device_count, 16);
        });
    }
}
