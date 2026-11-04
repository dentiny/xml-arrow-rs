# xml-arrow-rs

A Rust library for converting XML records into Apache Arrow `RecordBatch` values.
Column names, XML paths, types, and nullability are explicit. Conversion performs
no filesystem or network I/O.

This checkout implements **PR 1: scalar conversion with an explicit schema**.
See [MILESTONES.md](MILESTONES.md) for the remaining PRs.

## Convert records

```rust
use arrow_schema::DataType;
use xml_arrow::{XmlBatchBuilder, XmlField, XmlSchema};

let schema = XmlSchema::new(vec![
    XmlField::new("id", "@id", DataType::Int64, false),
    XmlField::new("title", "title", DataType::Utf8, false),
    XmlField::new("price", "price", DataType::Float64, true),
])?;
let mut builder = XmlBatchBuilder::new(schema, 1024)?;
builder.append_xml(
    r#"<book id="7"><title>Rust &amp; Arrow</title><price>29.5</price></book>"#,
)?;
let batch = builder.flush()?.unwrap();
assert_eq!(batch.num_rows(), 1);
assert_eq!(batch.num_columns(), 3);
```

The output is one row: `id = 7`, `title = "Rust & Arrow"`, `price = 29.5`.
Each `append_xml` call receives one complete record element, not an entire
multi-record document. Flush when `builder.is_full()` before appending again;
flush once more at end of input to emit a partial batch. Empty flushes return
`None`. Errors leave earlier rows intact and the builder reusable.

Run the in-memory Rust example:

```sh
cargo run --example convert -p xml-arrow
```

## Select columns

```rust,ignore
// Given the schema above, output title followed by id. Skip price conversion.
let builder = XmlBatchBuilder::new_projected(schema.clone(), 1024, &[1, 0])?;
// Count records without output columns. XML syntax is still checked.
let counter = XmlBatchBuilder::new_projected(schema, 1024, &[])?;
```

Projection uses zero-based schema indices, preserves requested order, and rejects
duplicates or out-of-range indices. Required values and scalar conversion are
checked only for selected columns.

## Scope

- `Utf8`, `Boolean`, `Int64`, and finite `Float64` columns.
- Relative descendant paths, terminal attributes, and record text (`.`).
- Missing nullable fields become null; present empty strings remain empty.
- Predefined/numeric XML references, CDATA, Unicode, and XML 1.0 line endings.
- Selected scalar elements cannot repeat or contain child elements.
- Unmapped content is ignored after XML parsing and input-contract checks.
- XML parsing, entity decoding, and normalization are delegated to `roxmltree`.
- Inputs are decoded Rust strings; declaration metadata does not change decoding.
- DTDs, custom entities, and namespace mapping are not supported.
- No nested Arrow types, automatic schema inference, record-path reader, storage
  integration, CLI, CI, or built-in IPC/Parquet export at this stage.

Batch size limits **rows, not bytes**. Callers supply complete records in memory;
the converter builds a read-only tree for one record during conversion, then
retains only the pending Arrow columns. This stage does not enforce application
record-size or depth limits. `Utf8` uses
Arrow's 32-bit string offsets; appends exceeding that format limit return an error.

See the [schema contract](docs/schema.md) for exact conversion and error behavior.
Future asynchronous and OpenDAL APIs will live in a separate `xml-io` crate.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Dependencies use Arrow 58, matching the local `atif-arrow` reference project.
Licensed under [MIT](LICENSE).
