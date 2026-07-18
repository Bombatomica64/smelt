# es-toolkit transpile profiling

Quick profiling of the Smelt **transpile** step (TS → Rust emission) on es-toolkit,
separate from the downstream `cargo build` of the generated crate. Corpus: es-toolkit
at the pinned CI ref (`e008a2818`), 1223 `.ts` files under `src/`, `build = false`.

## Phase breakdown (`SMELT_TIMINGS=1 smelt build`)

| phase | release | debug | share |
|---|---|---|---|
| `manifest.lower` (frontend: read/closure/specialize/frontend_lower) | 0.81s | 28.6s | ~10% |
| `mir.lower_hir` + `mir.optimize` + `mir.validate` | 0.14s | 4.7s | ~2% |
| **`rust.emit_crate`** | **6.71s** | **121.2s** | **~87%** |
| total transpile | ~7.7s | ~155s | 100% |

The debug numbers are what "felt slow" (~2.5 min). Rust emission dominates in both
builds; the frontend/MIR are not the problem.

## Where the 87% goes (perf, 7558 DWARF samples, release)

Flat self-time — see `blocker-logs/estoolkit-transpile-flamegraph.svg`:

| self % | symbol |
|---|---|
| 23.3% | `control_flow::…::block_reaches_target_avoiding` |
| 12.2% | `BuildHasher::hash_one` |
| 6.3%  | `HashMap::insert` |
| 5.9%  | `RawTable::reserve_rehash` |
| 5.5%  | `sip::Hasher::write` |
| 4.1% / 4.1% | `malloc` / `free` |
| 3.8%  | `control_flow::control_flow_successors` |
| 2.3%  | `cfg_queries::…::block_can_reach` |

The four hashing rows (`hash_one` + `insert` + `reserve_rehash` + `sip::write` ≈ **30%**)
are the `HashSet<BlockId>` visited-set churn inside the reachability DFS. Reachability
analysis over the CFG is therefore **~65%+ of the entire transpile**.

## Root causes (two, compounding) — `crates/smelt-codegen-rust/src/emitter/control_flow.rs`

1. **Linear block lookup inside a recursive DFS.** `block_reaches_target_avoiding`
   (and `block_reaches_target`) resolve each successor with a full linear scan:

   ```rust
   let Some(block) = self.function.blocks.iter().find(|block| block.id == block_id) else { … };
   ```

   That is O(blocks) per DFS node, and the DFS is re-run per branch/loop while
   structuring control flow → super-linear per function. es-toolkit's large generated
   functions blow this up.

2. **No memoization + SipHash visited sets.** The DFS is not cached (unlike the
   sibling `block_eventually_terminates`, which already uses `termination_cache`), and
   every call allocates a fresh `&mut HashSet::new()` — **29 such sites** in this file —
   using the default DoS-resistant **SipHash**. `BlockId(pub u32)` keys don't need it.

## Proposed fixes (low-risk, no lowering/codegen semantics change)

- **O(1) block lookup:** build a `Vec<&BasicBlock>` (or `HashMap<BlockId, usize>`)
  index once per `FunctionEmitter`; `BlockId` is a dense `u32`. Removes the linear scan.
- **Cheap visited sets:** replace `HashSet<BlockId>` visited/avoid with a `Vec<bool>`
  bitset indexed by block id (dense), or at minimum swap to `FxHashSet` (rustc-hash is
  already in the dependency tree via ruff). Removes the ~30% SipHash cost.
- **Memoize `block_reaches_target_avoiding`** the same way `termination_cache` /
  `loop_exit_cache` already work in `emitter/mod.rs`, keyed by `(block, target)` within
  the active avoid context.

Expected effect: removes most of the ~65% reachability cost — release transpile ~7.7s → a
few seconds, and the debug/CI path (what felt slow) drops proportionally.

Minor: ~4% is `write_if_changed`'s `readlink`/`openat` (reading existing outputs to
diff) — negligible, leave it.

## Fix applied — result: `emit_crate` 6.71s → 1.71s (−75%)

Two behavior-preserving changes (all 674 codegen snapshot tests still pass; output is
byte-identical — the lone `emits_regex_find_with_erased_haystack_string_coercion`
failure pre-dates this work on the branch). Release `emit_crate`, es-toolkit:

| stage | `emit_crate` | vs original |
|---|---|---|
| original | 6.71s | — |
| **A** — O(1) block lookup | 2.61s | −61% |
| **A + B** — + `FxHashSet` visited sets | **1.71s** | **−75%** |

- **A. O(1) block lookup.** `block_reaches_target_avoiding`, `block_reaches_target`,
  `block_can_repeat`, `block_can_reach`, and the `local_analysis` walk resolved each
  `BlockId` with a linear `blocks.iter().find(|b| b.id == id)`. Routed all five through
  the existing O(1) `block()` helper (`blocks.get(id.0)`), valid because `push_block`
  guarantees `blocks[i].id == BlockId(i)`. Collapsed the 23% self-time scan.
- **B. `FxHashSet` visited sets.** The visited/visiting/avoid sets are keyed by dense
  `u32` `BlockId`s but used the default SipHash (~40% of the post-A time). Added a
  `pub(crate) type BlockIdSet = rustc_hash::FxHashSet<BlockId>` alias
  (`emitter/mod.rs`) and switched every reachability set to it. SipHash cluster
  (`hash_one`+`sip::write`) dropped from ~40% to <3%.

Flamegraphs: `estoolkit-transpile-flamegraph-before.svg` /
`estoolkit-transpile-flamegraph-after.svg`.

### Remaining tail (not yet done — higher effort/risk)

After A+B the residual is allocation churn: `reserve_rehash`/`malloc`/`free` from
building a fresh visited set per DFS call (~20%) and `control_flow_successors`
allocating a `Vec` per call (~4%). Closing it means either a `Vec<bool>` bitset visited
set (rewrites ~29 `.insert()` sites and the `if !visited.insert(id)` idiom) or a
`SmallVec` successor return — both deferred pending sign-off.

## Reproduce

```bash
# corpus (same as CI)
ref=$(python3 -c "import json;print(next(l['ref'] for l in json.load(open('.github/compat/libraries.json'))['libraries'] if l['name']=='es-toolkit'))")
git clone --filter=blob:none https://github.com/toss/es-toolkit.git target/compat-repos/es-toolkit
git -C target/compat-repos/es-toolkit checkout "$ref"
cp -R .github/compat/es-toolkit/. target/compat-repos/es-toolkit/

# phase timings
SMELT_TIMINGS=1 ./target/release/smelt --manifest-path target/compat-repos/es-toolkit/Smelt.toml build

# flamegraph (needs kernel.perf_event_paranoid<=1)
CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo build --release --bin smelt
perf record -F 997 --call-graph dwarf,16384 -g -o perf.data -- \
  ./target/release/smelt --manifest-path target/compat-repos/es-toolkit/Smelt.toml build
flamegraph --perfdata perf.data -o blocker-logs/estoolkit-transpile-flamegraph.svg
```
