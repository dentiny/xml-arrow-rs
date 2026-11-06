# xml-arrow-rs

Convert XML records into Apache Arrow `RecordBatch` values in Rust. Define your
column mappings explicitly, or infer a schema from a bounded sample.

| Crate | Use it for |
| --- | --- |
| `xml-arrow` | XML parsing, schema mappings, inference, and Arrow batches. |
| `xml-io` | Opening local files, file URLs, and objects through OpenDAL. |

## Quick start

Add the library and its companion types to your application:

```toml
[dependencies]
xml-arrow = "0.2"
arrow-schema = "59.3.0"
futures-util = { version = "0.3", features = ["io"] }
futures-executor = "0.3"
```

Choose the record path and map fields relative to each record. Here,
`/catalog/book` selects one row per `<book>`, `@id` reads its attribute, and
`title` and `price` read child elements. The last argument to `XmlField::new`
sets nullability.

```rust
use arrow_schema::DataType::{Float64, Int64, Utf8};
use futures_util::io::Cursor;
use xml_arrow::{ReaderOptions, XmlField, XmlReader, XmlSchema};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema = XmlSchema::new(vec![
        XmlField::new("id", "@id", Int64, false),
        XmlField::new("title", "title", Utf8, false),
        XmlField::new("price", "price", Float64, true),
    ])?;
    let input = Cursor::new(br#"<catalog>
        <book id="7"><title>Rust &amp; Arrow</title><price>29.5</price></book>
        <book id="8"><title>XML</title></book>
    </catalog>"#);
    let mut reader = XmlReader::new(
        input, schema, "/catalog/book", ReaderOptions::default(),
    )?;
    futures_executor::block_on(async {
        while let Some(batch) = reader.next_batch().await? {
            println!("{} rows, {} columns", batch.num_rows(), batch.num_columns());
        }
        xml_arrow::ReaderResult::Ok(())
    })?;
    Ok(())
}
```

This produces one batch with two rows and three columns:

| id | title | price |
| ---: | --- | ---: |
| 7 | Rust & Arrow | 29.5 |
| 8 | XML | null |

`XmlReader` accepts `futures::io::AsyncBufRead + Unpin + Send`. In-memory reading
works with the futures executor, as above. Process each batch in the loop to
release its memory before reading the next; `reader.into_stream()` provides the
same batches as a futures stream.

## Read a file and infer its schema

To read a local `books.xml` with the same `/catalog/book` structure, also add
these entries under `[dependencies]`:

```toml
xml-io = "0.2"
tokio = { version = "1", features = ["macros", "rt"] }
```

This complete example uses Tokio for filesystem access and infers the columns:

```rust
use xml_arrow::{InferenceOptions, ReaderOptions, XmlReader};
use xml_io::file::XmlFile;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = XmlFile::from_uri("books.xml")?;
    let mut reader = XmlReader::infer(
        file.open().await?,
        "/catalog/book",
        &[],
        ReaderOptions::default(),
        InferenceOptions::default(),
    ).await?;
    println!("{}", reader.schema());
    while let Some(batch) = reader.next_batch().await? {
        println!("{} rows", batch.num_rows());
    }
    Ok(())
}
```

To supply known columns instead, pass the schema to
`XmlReader::new(file.open().await?, schema, "/catalog/book", options)`.

`XmlFile::from_uri` accepts local paths and `file://` URLs. Relative paths are
fixed against the current directory at construction; `..` is resolved when
opened, following filesystem symlinks. Each `open()` starts an independent read.
For other storage services, use `XmlFile::new(operator, "object/key.xml")` with
your configured OpenDAL operator. Its credentials, root, transport, and layers
are preserved. Import `opendal` directly when configuring an operator.

Inference samples up to **128 records or 8 MiB** by default, then replays the
sample before continuing on the same input. It works with non-seekable streams.
All inferred fields are nullable; attribute columns use names such as `@id`.
Integers and floats merge into `Float64`; other scalar type mixtures become
`Utf8`. Empty scalars and nil-only fields also use `Utf8`. Repeated siblings
become lists and nested elements become structs.

The inferred schema stays fixed. Later new fields are ignored; values that
violate mapped types or repetition rules return conversion errors. Set
`InferenceOptions::max_records` and `max_bytes` to change the sample bounds.
One additional complete record may be retained at the byte boundary, bounded
by `max_record_bytes`; a first record larger than the sample budget is an error.

## Map XML fields

Column names are your output names; XML paths determine where values come from.
Paths are relative to the selected record or enclosing struct/list item.

| Field path | Reads |
| --- | --- |
| `@id` | Attribute on the current element. |
| `title` | Direct child element. |
| `author/name` | A nested element. |
| `author/@id` | Attribute on a nested element. |
| `.` | Direct text of the current element, including CDATA. |
| `b:title` | A child in the namespace bound to `b`. |

These are case-sensitive paths with terminal attributes, rather than XPath
expressions. Wildcards, predicates, `..`, and absolute field paths are rejected.
The record path passed to `XmlReader` is absolute and includes the document root.

Supported types are `Utf8`, `Boolean`, `Int64`, finite `Float64`, `Struct`, and
`List`. Numeric and boolean conversion trims XML whitespace; `Boolean` accepts
`true`, `false`, `1`, and `0`. Strings preserve normalized XML text. A missing
nullable value becomes null, while `<title/>` is a present empty string. Empty
or invalid numeric values still fail when nullable. Repeated scalar elements
require a list mapping; scalar text cannot contain child elements.

### Nested values and namespaces

Use `structure` for one nested element and `list` for a container of repeated
items. Schema prefixes match namespace URIs, independently of input prefixes:

```rust
use arrow_schema::DataType::{Int64, Utf8};
use xml_arrow::{NamespaceBinding, XmlBatchBuilder, XmlField, XmlSchema};

let schema = XmlSchema::with_namespaces(
    vec![
        XmlField::structure("author", "b:author", true, vec![
            XmlField::new("id", "@id", Int64, false),
            XmlField::new("name", "b:name", Utf8, false),
        ]),
        XmlField::list("tags", "b:tags", true,
            XmlField::new("item", "b:tag", Utf8, true)),
    ],
    &[NamespaceBinding { prefix: "b", uri: "urn:books" }],
)?;
let mut builder = XmlBatchBuilder::new(schema, 1024)?;
builder.append_xml(r#"<book xmlns="urn:books">
    <author id="7"><name>Jane</name></author>
    <tags><tag>Rust</tag><tag>Arrow</tag></tags>
</book>"#)?;
let batch = builder.flush()?.unwrap();
assert_eq!(batch.num_rows(), 1);
```

The row contains `author = {id: 7, name: "Jane"}` and
`tags = ["Rust", "Arrow"]`. Child paths are relative to their parent. An empty
list container yields `[]`; a missing nullable container yields null. For
unwrapped repeated elements, use `.` as the list container path. Items may be
scalars, structs, or lists.

Bind prefixes the same way for namespaced record paths, such as
`/b:catalog/b:book`. Pass bindings as the third argument to `XmlReader::infer`;
unbound sampled URIs receive generated prefixes such as `ns0`. Inferred fields
sort by URI (no namespace first), then name. Unprefixed paths match elements
without a namespace; a default namespace does not apply to unprefixed attributes.

`xsi:nil="true"` or `"1"` yields null for a nullable mapped element. The prefix
can differ, but its URI must be `http://www.w3.org/2001/XMLSchema-instance`.
Required fields reject nil. Nil elements allow attributes and whitespace, with
no child elements or non-whitespace text. Structs/lists reject mixed text and
child elements.

### Select columns

For the quick-start schema `[id, title, price]`, request title followed by id:

```rust
use xml_arrow::ReaderOptions;

let options = ReaderOptions {
    projection: Some(vec![1, 0]),
    ..Default::default()
};
```

Pass these options to `XmlReader::new`. With `infer`, indices address the inferred
field order. For manual record conversion,
use `XmlBatchBuilder::new_projected(schema, 1024, &[1, 0])`. Only selected fields
are converted; selected record XML syntax is still checked. An empty projection counts rows
without output columns. Duplicate or out-of-range indices are errors.

## Convert individual records

If you already have complete XML record strings, use `XmlBatchBuilder` directly:

```rust
use arrow_schema::DataType::Int64;
use xml_arrow::{XmlBatchBuilder, XmlField, XmlSchema};

let schema = XmlSchema::new(vec![XmlField::new("id", "@id", Int64, false)])?;
let mut builder = XmlBatchBuilder::new(schema, 1024)?;
builder.append_xml(r#"<book id="7"/>"#)?;
let batch = builder.flush()?.unwrap();
assert_eq!(batch.num_rows(), 1);
```

Each append accepts one complete element. Flush when `is_full()` before adding
another record, then flush once at EOF for remaining rows. An empty flush returns
`None`. A failed append leaves earlier rows intact and the builder reusable.
This builder limits rows; use `XmlReader` for document-level byte/depth limits.

## Limits

`ReaderOptions` defaults:

| Option | Default |
| --- | ---: |
| `batch_size` | 1024 rows |
| `max_record_bytes` | 8 MiB per record or XML event |
| `max_batch_bytes` | 64 MiB of serialized records |
| `max_depth` | 128 elements |
| `projection` | All columns |

Depth and event-size limits apply to skipped content too. `max_batch_bytes` must be at least
`max_record_bytes`; byte limits count XML, including inherited namespaces, rather
than Arrow memory. One lookahead record and caller read-ahead buffers may also
be retained. `Utf8` and `List` use Arrow's 32-bit offsets.

Inputs must be UTF-8; XML encoding declarations do not transcode input. XML
references, CDATA, and Unicode are supported. DTDs and custom entities are rejected.
Selected records are parsed and validated in full, including unmapped fields.
Outside selected records, content is only tokenized by quick-xml to locate record
boundaries and preserve document structure and event/depth limits. It is not
reparsed with roxmltree: invalid attribute values or entity references in skipped
content may therefore go unreported.

Licensed under [MIT](LICENSE).
