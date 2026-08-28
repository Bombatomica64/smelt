# Library benchmarks: hand-written TypeScript vs Smelt-generated Rust

This directory measures what Smelt's output actually costs at runtime, on the two
libraries that transpile end to end today: [remeda](https://github.com/remeda/remeda)
and [es-toolkit](https://github.com/toss/es-toolkit).

For each library, the same operation is benchmarked twice — once as the original
TypeScript running on Node, once as the Rust that `smelt build` emits from that same
TypeScript — and both throughput (ops/s) and process footprint (peak RSS, CPU
seconds) are recorded.

The point is not to win a benchmark. It is to answer, per operation, the question in
`CLAUDE.md`: how close is the output to what a team rewriting this by hand in Rust
would have produced? A row where the generated Rust is *slower* than V8 is a row
where the answer is "not close yet", and it names the construct responsible.

## Reproducing

```sh
# 1. transpile both libraries at their pinned refs and build the bench binaries
python3 benchmarks/prepare.py

# 2. run every case on both sides (~40 min for 3 repeats; use --quick to smoke-test)
python3 benchmarks/run.py --repeats 3

# 3. render the Markdown report
python3 benchmarks/report.py
```

`prepare.py` also refreshes the bundled TypeScript in `benchmarks/ts/vendor/` if it is
missing; regenerate it manually with:

```sh
bun build target/compat-repos/remeda/packages/remeda/src/index.ts \
  --target=node --format=esm --outfile benchmarks/ts/vendor/remeda.mjs
bun build target/compat-repos/es-toolkit/src/index.ts \
  --target=node --format=esm --outfile benchmarks/ts/vendor/es-toolkit.mjs
```

Bun is used only as a bundler; the benchmark itself always runs on Node, so both
sides are compared against one JavaScript engine.

## Method

**Same source on both sides.** The TypeScript measured is the library's own source at
the pinned ref, bundled to ESM — not the published npm build. The Rust measured is
what `smelt build` emits from that identical source tree. Neither side is hand-tuned.

**Same data.** `benchmarks/ts/harness.mjs` and `benchmarks/rust/smelt_bench_harness.rs`
are twins: the same xorshift32 PRNG, the same seeds, the same vocabulary, the same
record shape. Inputs are built once, outside the timed region.

**Same protocol.** Warm up for 500 ms; then collect ~1 ms batches until 3 s have
elapsed and at least 10 batches exist, with a 20 s hard ceiling for cases where one
op takes seconds. The reported figure is the *median* batch, converted to ops/s.
Both harnesses implement this identically, down to the batch sizing.

**Verified parity.** Every case folds its result into a 32-bit checksum using
identical wrapping arithmetic on both sides. The report publishes a row only when the
two checksums agree; a mismatch means the two sides did not compute the same answer,
so comparing their speed would be meaningless. The checksum fold runs exactly once,
*outside* the timed region — walking a 10,000-element result is real work, and
charging it to the library would flatter whichever side walks structures faster.

**One case per process.** Peak RSS is a process-lifetime high-water mark, so sharing
a process between cases would attribute the largest case's memory to all of them.
Running each case alone also gives every case a cold JIT and a cold allocator. Both
runners enforce this.

**Footprint from one source.** Both sides read `/proc/self/status` (`VmHWM`, `VmRSS`)
and `/proc/self/stat` (utime, stime) rather than each using its own runtime's memory
API, which would be measuring two different things and calling them one.

**Noise.** The whole sweep is repeated (default 3×) with TS and Rust interleaved, and
the *fastest* observation of each side is reported: contention on a shared machine can
only make a run slower, so the minimum is the least-biased estimate available without
a dedicated quiet box. The report includes a per-row best/worst spread so the reader
can see how noisy the machine was.

## Layout

| Path | What it is |
| --- | --- |
| `prepare.py` | clones both libraries at pinned refs, transpiles, injects the bench modules, builds |
| `run.py` | runs every case on both sides, one process per case, writes `results/results.json` |
| `report.py` | renders `RESULTS.md` from that JSON |
| `ts/harness.mjs` | data generation, checksums, footprint, timing protocol (JS side) |
| `ts/cases.mjs` | the case table, shared by both libraries |
| `ts/run.mjs` | per-process Node runner |
| `rust/smelt_bench_harness.rs` | the twin of `harness.mjs` |
| `rust/smelt_bench_cases_*.rs` | per-library case tables (Rust side) |
| `rust/smelt_bench_main.rs` | per-process Rust runner |
| `smelt/*.Smelt.toml` | the bench manifests (compat fixtures minus test sources) |

### Why the bench code is injected rather than a separate crate

Everything the emitter produces — including `SmeltList`, `SmeltUnknown` and the
generated functions themselves — is `pub(crate)`, so an external crate cannot call it.
Rather than patch visibility (which would change the code under measurement),
`prepare.py` copies three modules into the generated `src/` and replaces the generated
`fn main() {}`. The generated files themselves are left byte-for-byte as emitted, which
also preserves the mtimes that let Cargo reuse incremental artifacts.

The generated function names carry emitter-assigned indices (`chunk_15`, `uniq_64`)
that shift whenever the module graph changes, so `prepare.py` discovers them and emits
`smelt_bench_entry.rs`, a table of stable `entry_*` aliases. The case files never name
a generated symbol directly.

## Reading the results

`RESULTS.md` reports **Rust / TS**, so `2.00×` means the generated Rust does twice the
throughput and `0.10×` means it does a tenth.

Two structural caveats:

1. **The two libraries are not comparable to each other.** remeda's public surface is
   built on `purry`, a runtime data-first/data-last dispatcher that inspects
   `arguments` to decide what it was called with. Smelt lowers that faithfully, so
   every remeda entry point has the shape
   `fn(args: SmeltList<SmeltUnknown>) -> Result<SmeltUnknown, _>`: the arguments are
   erased because in the source they genuinely are. es-toolkit's exports are ordinary
   typed functions, and come out with typed — often generic — Rust signatures. That is
   a difference between the two libraries' API designs, and only indirectly a
   difference in Smelt.

2. **`*_typed` rows have no TypeScript twin.** Where es-toolkit's generated function is
   generic (`fn f<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown>`), the
   benchmark registers two instantiations: `T = SmeltUnknown`, which is what an erased
   caller gets, and `T = f64`, which is what a caller holding a real `number[]` gets.
   The gap between those two rows is the price of erasure, measured directly, with no
   JavaScript involved on either side.
