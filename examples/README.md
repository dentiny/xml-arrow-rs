# Rust examples

Run from the repository root:

| Command | Demonstrates |
| --- | --- |
| `cargo run -p xml-arrow --example convert` | Explicit scalar mappings, record conversion, flushing, and reading Arrow values. |
| `cargo run -p xml-arrow --example stream` | Incremental batches, namespace bindings, and projected column order. |
| `cargo run -p xml-arrow --example infer` | Bounded inference and consuming the reader as a batch stream. |
| `cargo run -p xml-io --example storage` | Local paths and configured OpenDAL operators, using the checked-in Maven XML. |

`convert`, `stream`, and `infer` use in-memory inputs with the futures executor.
`storage` uses Tokio for local filesystem access and reads the fixture through
both `XmlFile` constructors. It resolves the fixture relative to the crate, so
the executable can run from another working directory too.

Use `next_batch()` to process and release batches incrementally. The small
`infer` example collects its two rows to demonstrate `into_stream()`; collecting
a large document retains all output batches.

The [README](../README.md) explains field mappings, nested values, namespaces,
inference, input limits, and error behavior.
