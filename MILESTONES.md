# XML to Arrow milestones

Implement one reviewable PR at a time, in dependency order. This checkout
implements milestones 1–3. Fixture-only PRs can land between functional
milestones, so milestone numbers are independent of GitHub PR numbers.
No CI or CLI is planned.

| Milestone | Deliverable | Acceptance |
| --- | --- | --- |
| 1 | Workspace, explicit scalar schema, field paths, atomic batch builder, projection | All four scalar types, missing/empty values, references/CDATA, projection order, zero-column row counts, and failed-append recovery pass. |
| 2 | Nested Struct/List, namespace URI matching, xsi:nil | Empty lists, absent parents, nested nulls, repeated data, and mixed-content rules pass. |
| 3 | xml-arrow async reader over futures::io::AsyncBufRead, record paths, next_batch and stream | Incremental reads, split tags/UTF-8, cancellation/resume, first-error termination, depth/record/batch byte limits pass. |
| 4 | xml-io storage access: reopenable XmlFile, local/file URLs, configured OpenDAL operators, optional gzip | Local/memory storage, repeat opens, single/multiple gzip members, and provenance pass. |
| 5 | Bounded schema inference | Deterministic merging, row/byte sampling limits, replay on non-seekable input, and later-schema conflicts pass. |
| 6 | Benchmarks and complete usage documentation | Flat/wide/nested/large inputs have throughput and memory baselines; examples cover implemented public APIs. |

Each PR includes its own tests and documentation. Run formatting, clippy, and
workspace tests locally. XML parsing, record selection, asynchronous reading, and Arrow conversion stay
in `xml-arrow`. `xml-io` is reserved for storage access, starting in milestone 4.
Raw XML retention,
if introduced, is opt-in.
