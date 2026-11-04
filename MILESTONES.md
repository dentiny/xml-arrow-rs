# XML to Arrow milestones

Implement one reviewable PR at a time, in dependency order. This checkout
implements PR 1. No CI or CLI is planned.

| PR | Deliverable | Acceptance |
| --- | --- | --- |
| 1 | Workspace, explicit scalar schema, field paths, atomic batch builder, projection | All four scalar types, missing/empty values, references/CDATA, projection order, zero-column row counts, and failed-append recovery pass. |
| 2 | Nested Struct/List, namespace URI matching, xsi:nil | Empty lists, absent parents, nested nulls, repeated data, and mixed-content rules pass. |
| 3 | xml-io async reader over futures::io::AsyncBufRead, record paths, next_batch and stream | Incremental reads, split tags/UTF-8, cancellation/resume, first-error termination, depth/record/batch byte limits pass. |
| 4 | Reopenable XmlFile, local/file URLs, configured OpenDAL operators, optional gzip | Local/memory storage, repeat opens, single/multiple gzip members, and provenance pass. |
| 5 | Bounded schema inference | Deterministic merging, row/byte sampling limits, replay on non-seekable input, and later-schema conflicts pass. |
| 6 | Benchmarks and complete usage documentation | Flat/wide/nested/large inputs have throughput and memory baselines; examples cover implemented public APIs. |

Each PR includes its own tests and documentation. Run formatting, clippy, and
workspace tests locally. Core conversion stays in `xml-arrow`; storage and async
input handling belong to `xml-io` starting in PR 3. No empty placeholder I/O crate
is added in PR 1. Raw XML retention, if introduced, is opt-in.
