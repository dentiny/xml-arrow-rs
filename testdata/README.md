# Real XML fixtures

These are complete, unmodified configuration/topology files from local checkouts
of public projects. They are real project files rather than generated XML test
snippets or a large document corpus. Tests embed these snapshots and run offline.

## LanceDB Maven POM

- File: `lancedb-pom.xml` (15,205 bytes).
- Source: [lancedb/lancedb, java/pom.xml](https://github.com/lancedb/lancedb/blob/1ea47a50559f02a0251d9d43237259b559606abd/java/pom.xml).
- Git blob: `33446040b5e0532c945ee3f4c718563dca5250a3`.
- SHA-256: `b4f82c7efc8b448cd59b1fa4ed84473cc5658e716c30b9adc76f7f82cb592c8c`.
- License: [Apache-2.0](licenses/Apache-2.0.txt).
- Coverage: default XML namespace, dotted element names, nested properties,
  dependency/module lists, optional fields, and literal Maven placeholders.

## Azure NDv5 hardware topology

- File: `azure-ndv5-topology.xml` (3,395 bytes).
- Source: [NVIDIA/NVSentinel, ndv5-topo.xml](https://github.com/NVIDIA/NVSentinel/blob/09cf75f8bc27dc8dcafa67354cf879f7d0ff1858/distros/kubernetes/nvsentinel/charts/preflight/files/ndv5-topo.xml).
- Git blob: `02fa383a75775f7b55a235a2926db6aedc5121dd`.
- SHA-256: `7d29707d077c654121988fad48b3fa70df7a3e3d36bd00abb2ba47696ffed5e1`.
- The source comment credits [Azure/azhpc-images](https://github.com/Azure/azhpc-images/blob/master/topology/ndv5-topo.xml) and is retained. NVSentinel uses
  [Apache-2.0](licenses/Apache-2.0.txt); Azure's original project uses
  [MIT, copyright Microsoft Corporation](licenses/Azure-MIT.txt).
- Coverage: attributes and nested CPU → PCI bridge → device lists, including
  exact counts (2 CPUs, 8 bridges, 16 devices) and integer conversion.

Both local files were checked against their public upstream Git blobs before
copying. Refresh the source revision, hashes, and expected test values together
when deliberately updating a fixture.

Run `cargo test -p xml-arrow --test testdata` from the repository checkout.
The fixture integration test is excluded from the published crate because its
inputs live at the workspace root.
