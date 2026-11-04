# xml-arrow-rs

A Rust library for converting XML records into Apache Arrow `RecordBatch` values.
Column names, XML paths, types, and nullability are explicit. Conversion performs
no filesystem or network I/O.

This checkout implements **PRs 1–2: scalar and nested conversion with an explicit schema**.
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

## Nested fields and namespaces

```rust
let schema = XmlSchema::with_namespaces(vec![
    XmlField::structure("author", "b:author", true, vec![
        XmlField::new("id", "@id", DataType::Int64, false),
        XmlField::new("name", "b:name", DataType::Utf8, false),
    ]),
    XmlField::list("tags", "b:tags", true,
        XmlField::new("item", "b:tag", DataType::Utf8, true)),
], &[("b", "urn:books")])?;
let mut builder = XmlBatchBuilder::new(schema, 1024)?;
builder.append_xml(r#"<book xmlns="urn:books">
    <author id="7"><name>Jane</name></author>
    <tags><tag>Rust</tag><tag>Arrow</tag></tags>
</book>"#)?;
```

The `author` struct contains `{id: 7, name: "Jane"}` and `tags` is
`["Rust", "Arrow"]`. Schema prefixes match by URI, independently of the XML's
prefixes or default namespace. Unprefixed paths select names without a namespace;
unprefixed attributes such as `@id` stay outside the default namespace.

Struct child paths are relative to the selected parent. A list path selects one
container; its item path selects repeated values inside it. An existing empty
container becomes `[]`, while a missing nullable container becomes null. For
unwrapped repeated elements, use `.` as the list container path. Lists can contain
scalars, structs, or other lists.

`xsi:nil="true"` or `"1"` produces null when the mapped field is nullable. The
`xsi` prefix may differ, but its URI must be
`http://www.w3.org/2001/XMLSchema-instance`. Non-nullable fields reject nil.

## Scope

- `Utf8`, `Boolean`, `Int64`, finite `Float64`, `Struct`, and `List` columns.
- Relative descendant paths, terminal attributes, and record text (`.`).
- Missing nullable fields become null; present empty strings remain empty.
- Predefined/numeric XML references, CDATA, Unicode, and XML 1.0 line endings.
- Selected scalar elements cannot repeat or contain child elements.
- Unmapped content is ignored after XML parsing and input-contract checks.
- XML parsing, entity decoding, and normalization are delegated to `roxmltree`.
- Inputs are decoded Rust strings; declaration metadata does not change decoding.
- Namespace URI matching and nullable elements via `xsi:nil`.
- DTDs and custom entities are not supported.
- No automatic schema inference, record-path reader, storage
  integration, CLI, CI, or built-in IPC/Parquet export at this stage.

Batch size limits **rows, not bytes**. Callers supply complete records in memory;
the converter builds a read-only tree for one record during conversion, then
retains only the pending Arrow columns. This stage does not enforce application
record-size or depth limits. `Utf8` and `List` use 32-bit offsets; exceeding their
format limits returns an error before any columns are changed.

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
