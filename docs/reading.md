# Asynchronous document reading

`xml_arrow::XmlReader::new(input, schema, record_path, options)` accepts one UTF-8
XML document through `futures::io::AsyncBufRead + Unpin + Send`. Wrap an unbuffered
async reader in `futures_util::io::BufReader`. Input may borrow data for the
reader's lifetime. Construction validates configuration without consuming input.
Storage access belongs in the future `xml-io` crate; this reader only consumes
the supplied bytes. It does not require a Tokio runtime: `tokio-util` only adapts the input traits to
quick-xml's asynchronous parser.

`next_batch().await` returns `Result<Option<RecordBatch>>`. `into_stream()` returns
a `Send` stream of batch results. `schema()` returns the projected Arrow schema.
Projection uses the same root-column indices and ordering as `XmlBatchBuilder`;
an empty projection still counts records and validates XML syntax.

## Record selection and parsing

Record paths are absolute and include the document root, such as `/catalog/book`.
Only element steps are supported; no wildcards, predicates, or attributes.
Prefixes resolve through the supplied `XmlSchema` namespace bindings and match
by URI, independently of source prefixes. Unprefixed steps select names without
a namespace. For a default-namespaced document, bind its URI to a schema prefix
and use that prefix in the record path.

quick-xml reads XML events incrementally and selects complete record elements. Each selected element is serialized with
its inherited namespace declarations, then converted by `xml-arrow` using
roxmltree. Skipped tokens are also validated with roxmltree, which checks XML
names, characters, attributes, and references beyond quick-xml's structural
checks. DTDs and custom entities are rejected. XML declarations do not enable
transcoding. Only one document element is allowed; empty input is an error,
while a valid document with no selected records returns `None`.

## Limits and batching

| `ReaderOptions` field | Default | Meaning |
| --- | --- | --- |
| `batch_size` | 1024 | Maximum rows per batch. |
| `max_record_bytes` | 8 MiB | Maximum serialized record size, including inherited namespaces, and maximum input consumed per XML event. |
| `max_batch_bytes` | 64 MiB | Maximum sum of serialized record sizes in a batch; must be at least `max_record_bytes`. |
| `max_depth` | 128 | Maximum element depth anywhere in the document; valid values are `1..65535`. |
| `projection` | `None` | All columns, or root-column indices in output order. |

All size limits must be positive. Event limits apply to unselected comments,
text, and tags as well as selected records. Input accumulation stops at a limit
instead of first buffering an arbitrarily large event. The reader retains at
most one candidate record beyond the current batch when enforcing the batch
byte limit. Supplied input buffers may independently read ahead.

Byte accounting uses serialized XML with inherited namespaces; it does not
measure Arrow allocations or process memory. Arrow memory depends on the schema
and conversion. The core builder also checks Arrow's 32-bit string/list offsets.

## Cancellation, errors, and EOF

The internal stream owns the active parsing future. Dropping a pending
`next_batch()` future preserves partial tokens, partial records, and pending rows
for the next call. Dropping the reader or stream releases its input.

The first error discards the unfinished batch and ends the reader. Subsequent
calls return `None`; batches already returned remain valid. Conversion errors
carry the one-based selected record index, its opening-tag byte offset, and the
original `xml_arrow::Error`. XML/I/O errors preserve their original error source
and a source byte offset. Limit errors identify the limit and source position.

Reads stop when a batch is ready. Unread document tails are checked only when
consumed; callers must read to EOF to validate the entire document. An error in
a later batch does not retract batches already returned.
