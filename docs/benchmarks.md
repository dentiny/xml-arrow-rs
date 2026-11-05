# Reader benchmarks

Run all cases with:

```sh
cargo bench -p xml-arrow --bench reader
```

The benchmark uses Criterion with 10 samples, a one-second warmup, and a
two-second measurement window per case. Synthetic documents are deterministic
and generated outside timed iterations. Each iteration constructs a reader,
consumes the complete document, and drops each output batch. Schema and row-count
assertions remain enabled. Throughput counts input XML bytes, including the root.

`explicit` uses a supplied schema; `infer` includes sampling, schema construction,
and replay. Both produce the same Arrow schema and use default reader limits
and batch size (1024 rows). Inference uses its default 128-record / 8 MiB sample.
These measurements exclude storage access and input generation.

## Peak memory

Build once and use the executable path printed by Cargo:

```sh
cargo bench -p xml-arrow --bench reader --no-run
/usr/bin/time -l target/release/deps/reader-HASH --bench explicit/flat --exact --test
```

This runs one complete document conversion. Repeat in a fresh process for each
`explicit`/`infer` and `flat`/`wide`/`nested`/`large` combination. `--exact` selects one case; only that
case constructs a complete input document. On macOS, `maximum resident set size`
is bytes; divide by 1,048,576 for MiB. On Linux, use `/usr/bin/time -v` and divide
its maximum RSS in KiB by 1024 instead.

Peak RSS includes the resident input, fixture construction, Arrow batches,
inference replay records, allocator retention, and the benchmark harness. It is
a whole-process baseline, not a measurement of live Arrow allocations or an RSS
limit. Every process also constructs the small record templates and explicit
schemas for all four cases. The large case fits all 64 rows in one batch at the
default limits.

## Baseline

Measured on 2026-11-05 with Apple M5 Pro (arm64), 24 GiB RAM, macOS 26.5.2,
Rust 1.97.1, Arrow 58.4.1, and Criterion 0.8.2. Cargo's default release profile
was used. Conversion code is unchanged from `a43be27`.

| Case | Records | Input XML bytes | Shape |
| --- | ---: | ---: | --- |
| flat | 10,000 | 890,013 | Four scalar fields, including a decoded entity. |
| wide | 2,000 | 1,666,013 | 64 integer fields per record. |
| nested | 5,000 | 450,013 | A nested author struct and two repeated tags. |
| large | 64 | 16,779,597 | A 256 KiB text payload per record. |

Throughput uses Criterion's central time estimate (slope for linear sampling,
mean for flat sampling). RSS is the single-conversion measurement above.

| Case | Explicit MiB/s | Inference MiB/s | Explicit peak RSS MiB | Inference peak RSS MiB |
| --- | ---: | ---: | ---: | ---: |
| flat | 34.86 | 32.57 | 9.5 | 9.6 |
| wide | 18.46 | 18.67 | 11.3 | 11.5 |
| nested | 30.12 | 32.32 | 8.9 | 9.0 |
| large | 794.86 | 643.59 | 46.6 | 51.0 |

These are single-host observations; compare repeated runs under similar load
when assessing a code change. In particular, the small differences between
explicit and inferred readers do not establish a speedup from inference.

Repeated conversions have a different RSS profile. Fresh processes running
`--profile-time 1` instead of `--test` reached 1053.8 MiB (explicit) and 570.9 MiB
(inference) for `large`. That loop performs different iteration counts in each
mode, so its peaks are not an equal-work memory comparison. They include process
high-water behavior across repeated allocations; RSS alone does not distinguish
live allocations from allocator retention. A single-conversion peak therefore
does not bound a long-running process.
