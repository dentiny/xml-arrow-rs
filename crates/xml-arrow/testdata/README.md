# XML test fixtures

These files are handcrafted for this project's XML parsing tests. They contain
fictional data and are covered by the repository's MIT license. No public-project
XML snapshots, downloads or additional fixture licenses are needed.

- `catalog.xml`: five products under an inherited default namespace. Covers
  text/entity decoding, UTF-8, decimal values, missing nullable fields, list
  mappings, schema inference and record order across batches. A settings block
  covers dotted element names and literal placeholder text.
- `inventory.xml`: two warehouses, each containing two racks with two devices.
  Both rack and device levels use `<node>` deliberately, so the tests verify
  exact-depth record selection as well as nested structs/lists and numeric
  attributes. IDs and expected order are explicit.
- `numeric-boundaries.xml`: focused regression cases at the Float64 exact-integer
  boundary and the Int64 limits, including both sample orders and out-of-range
  integer literals.

`tests/fixtures.rs` reads the catalog and inventory with `XmlBatchBuilder` and
`XmlReader`; async tests read fixture bytes directly through `Cursor`. Dedicated
reader tests cover input fragmentation and cancellation separately.
`tests/numeric_inference.rs` exercises the numeric boundary fixture.

The XML files and integration tests are included in the crate package without
exclusions. Tests use the committed local inputs and require no network access.
Run `cargo test -p xml-arrow --test fixtures` or `cargo test --workspace`.
