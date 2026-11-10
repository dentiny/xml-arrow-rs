# Direct child list selection

The common single-element list selector used to allocate a `Vec<Source>` and
then a `Vec<Value>`. Visit matching children directly and collect only values.
Keep the general selector for attributes, self and multi-segment paths.

```sh
cargo build -p xml-arrow --release --example bench_lists
# Alternate saved baseline/candidate executables in separate processes.
target/release/examples/bench_lists 1024
target/release/examples/bench_lists 16
target/release/examples/bench_lists 1024 nested
```

Each two-second process converts 32-row batches. Five alternating A/B pairs on
macOS ARM64 gave median paired wall-time reductions of 5.8% (1024 direct items)
and 9.3% (16 items). The unchanged multi-segment control shifted 0.5%. One large
list pair regressed 32.8%, so the result is a local indication with substantial
noise, not a universal throughput claim. The change deterministically removes
an intermediate source collection on the direct-child path.

Raw samples are attached to https://github.com/dentiny/open-arrow-access/pull/228
under `benchmarks/matrix/results/xml-upstream.jsonl`. The measured baseline is
`1bf4fce`; the PR rebases onto `13572ef`, which extracts the existing XML whitespace
trim expression into a shared helper. The native test suite covers both selectors,
namespace matching, nil/empty items, mixed content and failed-append atomicity.
