# Benchmarks

Measured 2026-10-16 on the development laptop (16 threads, 30 GiB RAM, Linux, release build).

## Synthetic 1M-line project

`cargo run --release -p codescape-synth -- <dir> --lines 1000000` generates 40 Gradle modules, 7,610 Java classes
(including one 2,000-class package) and 10 mains. `cargo run --release -p codescape --bin scene_bench -- <dir>`
indexes it, opens it, and scripts a 300-step zoom from the whole map down to a single class.

| Measure | Result | Target |
|---|---|---|
| Lines indexed | 1,019,962 | 1M |
| Index time | 1.4 s | < 60 s |
| Index size | 6.4 MiB, 30% of 21.5 MiB source | ≤ 30% |
| Open + first scene (incl. checksum) | 4 ms | < 1 s |
| Scene build while zooming | worst 0.65 ms, p99 0.19 ms | < 16 ms per frame |
| Largest container layout (2,000 children) | 1 ms | < 100 ms |
| Viewer peak RSS (window open, deep zoom) | 165 MiB | < 500 MiB |
| Indexer peak RSS | 261 MiB | — |
| Zoom range exercised | 0.19× → 950× | whole map → one class |

Scene build time is the CPU work per frame; egui tessellation and GPU submission come on top and were not
separately timed.
