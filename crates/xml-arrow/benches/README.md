# Nested path selection

Run `cargo bench -p xml-arrow --bench nested_paths -- 100000` at least three times
per revision. Input generation and warm-up are outside the timer. The benchmark
includes XML DOM parsing, conversion, Arrow offset validation and batch creation;
it isolates those from streaming record discovery and storage. Use the
open-arrow-access pipeline benchmark to measure the end-to-end effect.

End-to-end open-arrow-access measurements used 32,768 records, two mapped fields,
128-byte text, 1024-row batches, seven scans per process and five alternating A/B
processes on macOS arm64 with Rust 1.97.1:

| Mapping/backend | Before ms | After ms |
| --- | ---: | ---: |
| Direct/memory control | 33.191 | 33.220 |
| Direct/HTTP replay control | 33.187 | 33.817 |
| Nested/memory | 54.038 | 49.242 |
| Nested/HTTP replay | 55.202 | 48.762 |

The nested workload improves by about 10–13%; no direct-path improvement is
claimed. HTTP is in-process protocol replay. The complete matrix and raw process
measurements are recorded in the open-arrow-access benchmark PR.
