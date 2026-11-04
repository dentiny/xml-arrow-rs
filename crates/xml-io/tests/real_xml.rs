use arrow_array::{cast::AsArray, types::Int64Type, Array};
use arrow_schema::DataType::{Int64, Utf8};
use futures_executor::block_on;
use futures_util::{
    io::{BufReader, Cursor},
    TryStreamExt,
};
use xml_arrow::{XmlField, XmlSchema};
use xml_io::{ReaderOptions, XmlReader};

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
