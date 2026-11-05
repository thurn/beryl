//! Benchmark support. `docs/benchmarking.md` explains the strategy; the `example_*`
//! benchmarks show it end to end.
//!
//! - [`rng`]: a fixed, seeded generator, so every run measures the same inputs.
//! - [`cores`]: asks macOS to run the benchmark thread on the fastest cores.
//! - [`abba`]: old-versus-new timing interleaved in one executable.
//! - [`example`]: the sample subject the `example_*` benchmarks measure.

pub mod abba;
pub mod cores;
pub mod example;
pub mod rng;
