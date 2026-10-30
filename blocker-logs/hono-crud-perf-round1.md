# hono_crud perf, round 1: generated Hono server vs Node

Branch `perf/hono-crud-speed`, based on `feat/hono-url-model` at `8643a3f1`. Yardstick:
`benchmarks/hono_crud` (`node-original` vs `rust-shipped`), method as in its README: server pinned
to core 2, `oha` 1.16 on cores 6–11, 5 s warm-up + 10 s at 32 keep-alive connections per scenario,
fresh process and fresh `todos.db` per run, variants interleaved, **median of 3 runs**.

This VM is shared and its absolute throughput drifts by up to ±25 % between sessions (Node's
`GET /todos/abc` measured 47–61 k req/s on the same day), so every comparison below is between
variants measured **interleaved in the same session**; ratios are the reliable figure.

## 0. The branch head did not serve

Before any measurement, the shipped example failed at `8643a3f1` in three ways (fixed in
`101d8905`, each at its source):

| Symptom | Cause | Fix |
| --- | --- | --- |
| E0308 compiling the crate | `record_slot_abi`: a callable read straight off the same declared slot of the same generic record (`Router<[unknown, RouterRoute]>` rebuilt from `options.router ?? new SmartRouter(..)`) was adapted a second time | skip the adapter when the field coercion passed the read through and both records instantiate the same declared slot |
| panic `RefCell already borrowed` at startup | a callable-object slot call through a reference class kept its `Ref` guard alive across the call (`router.0.borrow().get.clone().__smelt_call.clone().call(..)`), and Hono's `get` slot `borrow_mut()`s the same cell | bind the slot in its own `let` (the rule `indirect_call_text` already applies to plain function fields) |
| every route 404 | `const { strict, ...rest } = options` erased the `HonoOptions` struct through its generic `IntoSmeltUnknown`, which renders callable fields as `null`; the router passed through Hono's `#clone()` lost `add`/`match`, so `app.route('/todos', ..)` mounted nothing | the `...rest` copy of a nominal record uses the emitter's field-wise object erasure |

After these, `probe.sh` output is byte-identical to the committed `probe-rust-shipped.txt`, and
it stayed identical after every change below.

## 1. Baseline (this session)

| Scenario | node-original | rust-shipped @ head (+§0) | ratio |
| --- | ---: | ---: | ---: |
| `GET /todos` | 13,510 | 12,691 | 0.94× |
| `GET /todos/1` | 17,209 | 15,438 | 0.90× |
| `GET /todos/abc` | 47,928 | 24,909 | 0.52× |
| `GET /nope` | 46,342 | 41,177 | 0.89× |
| `PATCH /todos/1` | 7,830 | 11,389 | 1.45× |
| `POST /todos` | 53 | 57 | 1.08× |

`POST` is bound by SQLite's journaled write on both sides (~55 req/s, p50 ~400–700 ms); it is not
CPU work and no change here moves it.

## 2. Hotspots and fixes

Profiles: `perf record -g` (frame pointers, line tables) of the generated server under load on
`GET /todos/abc`, folded and walked from `smelt_http_dispatch`. Kernel networking (one `writev`
per response, loopback with nf_tables conntrack) is ~25–35 % of samples throughout and is the same
work Node does.

| # | Hotspot (share of samples when found) | Fix (commit) | Effect |
| --- | --- | --- | --- |
| 1 | `res ?? this.#notFoundHandler(c)` ran the not-found handler — a full `c.json(..)` response — on **every** request (5.5 %) | MIR lowers `??` with a branch when the fallback can do work; the fallback is lowered only in the nullish arm (`5f6bae55`). Also a correctness fix: `??` short-circuits in JavaScript | `/todos/abc` 24,909 → 27,520 |
| 2 | glibc `malloc`/`free` and internals: libc was ~28 % of samples | the example opts into the existing `[rust] allocator = "mimalloc"` (application config, see `GeneratedAllocator`) | `/todos/abc` 27,520 → 34,732; `/todos` and `/todos/1` overtake Node |
| 3 | eager erased snapshots: every erasure of the Hono `Context` (route handlers are stored as `unknown`) built ~20 fields, wrapping each class-field arrow in an erased adapter registered in the callable table, and the handler's adapter immediately recovered the same `Context` from its origin (~13 %); `Request`/`Response` erasures likewise (~4 %) | `lazy_view` (`c99715c2`): the field store of a program with erased-view origins sits behind `SmeltStoreCell`, whose `borrow()`/`borrow_mut()` build a pending snapshot first; only the origin slot bypasses it. Non-generic reference records and the fetch `Request`/`Response` erasures defer their snapshot | `/todos/abc` 34,126 → 45,646 (+34 %) |
| 4 | SipHash on the callable address table (~2.8 %) | folded-multiply hasher for address keys (`c99715c2`) | in #3's number |
| 5 | `url.charCodeAt(i)` cloned the whole string per character (Hono `getPath`) | borrow the receiver (`c99715c2`); ASCII prefix reads the byte directly (`c62ec062`) | in #3/#6 |
| 6 | per-request URL parse + re-serialize of `http://host/path` (~3 %); `SmeltRequest::default()` and every `Request`-typed field placeholder (`about:blank`) running the URL parser; seven `RequestInit` strings per request clone | conservative "already serialized" test skips the parser only when serialization is provably the identity; `about:blank` passes through; `RequestInit` shared behind an `Rc` (`c62ec062`, `c99715c2`) | `/todos/abc` 53,772 → 56,863 |
| 7 | optionals cloned twice on narrowing/coalescing reads (a regex match's captures twice per request in Hono's router); absent-optional defaults (fresh `Rc` adapters) built eagerly | borrow + one clone; `unwrap_or_else` for defaults (`e21601bd`) | `/todos/abc` 56,748 → 58,857 |
| 8 | finished server response copied into hyper (`into_wire_parts` cloned headers and body); header names lowercased twice | move the buffers out; `HeaderName` is lowercase already (`57291b58`) | within noise |

Tried and reverted: writing a fetch handler's non-promise answer synchronously instead of
through the per-request `spawn_local` task. `perf stat` showed the same instructions per request
(~137 k) but ~29 % more cycles, and `/todos/abc` fell 49.3 k → 39.0 k req/s. The deferred write is
kept; why the synchronous path stalls more is not understood yet.

## 3. Result

Final tree (`d1c4316f` code + regenerated goldens), interleaved, medians of 3; raw rows in
`benchmarks/hono_crud/results/bench-interleaved-perf-round1.tsv`, load averages 1.5–2.4 in
`…-load.txt`. `rust-base` is the branch head with §0's fixes (system allocator);
`rust-final-sys` is this branch's code with the system allocator, separating the code changes
from the allocator choice; `rust-shipped` is the example as it now ships (mimalloc).

| Scenario | node-original | rust-base | rust-final-sys | rust-shipped | shipped ÷ Node |
| --- | ---: | ---: | ---: | ---: | ---: |
| `GET /todos` | 17,779 | 14,753 | 19,246 | **23,569** | **1.33×** |
| `GET /todos/1` | 21,844 | 18,255 | 26,228 | **29,258** | **1.34×** |
| `GET /todos/abc` | 60,036 | 29,070 | 50,151 | 59,107 | 0.98× |
| `GET /nope` | 60,749 | 47,537 | 57,813 | **65,720** | **1.08×** |
| `PATCH /todos/1` | 8,034 | 13,323 | 18,205 | **20,901** | **2.60×** |
| `POST /todos` | 55 | 57 | 59 | 57 | 1.04× (SQLite-bound tie) |

p50/p99 latency, shipped vs Node: `GET /todos/abc` 0.54/0.61 ms vs 0.52/1.08 ms; `GET /todos`
1.35/1.46 ms vs 1.71/3.70 ms. Peak `VmHWM`: generated 15 MiB (system allocator) / 25 MiB
(mimalloc) vs Node 139–254 MiB. Every scenario answered its expected status on every run.

Code alone (system allocator, `rust-base` → `rust-final-sys`): +30 % `/todos`, +44 %
`/todos/1`, +73 % `/todos/abc`, +22 % `/nope`, +37 % `PATCH`. The allocator adds a further
+11–22 % on the reads.

`rust-shipped` beats Node on five of six scenarios. `GET /todos/abc` is 1.5 % behind (behind in
each of the three rounds, by 0.3–3 %); it is the one scenario not won, and §5 lists what is left
on that path.

## 4. Verification

Run locally on the final tree unless marked otherwise.

| Check | Result |
| --- | --- |
| `probe.sh` against the generated shipped server | byte-identical to the committed `results/probe-rust-shipped.txt` after §0 and after every change (c1–c6) |
| `cargo clippy --lib -p smelt-mir -p smelt-codegen-rust` | no errors; no warnings on changed lines except one pre-existing `value` shadowing in `emitter/core.rs` the diff touches |
| `cargo test -p smelt-mir --lib` | 69/69 |
| `cargo test -p smelt-codegen-rust --lib` | 1190/1190 (two expected-text tests updated, one corrected; below) |
| end-to-end goldens (`SMELT_UPDATE_EXAMPLE_RUST=1 … end_to_end`) | 3/3 after regeneration (every example built, run and diffed against its `expected.stdout`); one MIR golden (121) regenerated |
| examples SmeltUnknown report (`--fail-on-regression`) | avoidable erasure 0 → 0 |
| examples typedness (`--fail-on-regression`) | no program less typed; 121 +1 concrete value, baseline re-snapshotted |
| runtime fixtures (`build_runs_runtime_fixtures`) | 22/22 pass, each also checked against Node's own output by the harness's oracle (three new, below) |
| es-toolkit / remeda ratchets, full `cargo test`, `clippy --all-targets` | not run locally (left to CI) |
| shipped vitest suite as generated Rust | not run (as in the previous round) |

New regression evidence, each checked against a deliberate defect:

* runtime fixture `nullish_fallback_runs_only_when_selected` (Node 24 output): `??` runs its
  fallback once per nullish left operand, never for a present one, at an optional and an erased
  type, and for a method call on a class instance;
* runtime fixture `erased_reference_view_reads_live_fields` (Node 24 output): identity round
  trip, reads, own-key order and JSON through a deferred view. Making `SmeltStoreCell::borrow`
  skip materialization fails it ("missing field");
* runtime fixture `request_url_serialization` (Node 24 output): `request.url` for inputs on both
  sides of the "already serialized" test. Dropping the dot-segment guard fails it (lines 10–11).

Test expectations changed, with the reason:

* `emits_string_char_at_method` and the `string_method_surface_emission` snapshot pin the
  emitted text, which now borrows the receiver and carries the ASCII fast path.
* `branch_join_narrows_optional_reassigned_in_both_arms` asserted the absence of
  `from_index.clone().expect`. Narrowing an optional local is *rendered* as that checked read;
  the assertion held only because the read used to carry a redundant second `.clone()`, and with
  the join narrowing disabled the old assertion still passed (it never detected its defect). It
  now asserts the guarantee of the change that added it (`86856594`, clearing an E0308): both
  reads are narrowed and none falls back to `unwrap_or(0.0)`. Disabling the narrowing in
  `types_iface.rs` fails it.

## 5. What remains

* `GET /todos/abc` is the closest scenario. The remaining per-request work is spread thin; the
  biggest blocks are the router chain (~11 %: `SmartRouter` → `RegExpRouter` through erased
  record-slot adapters, a fancy-regex exec, and the match array's `index`/`input` stored in the
  thread-local named-property table), the route handler's `Context#json`/`#newResponse`
  (~6 %), and `Context` construction (~4 %: ~15 class-field arrows, `ContextOptions` rebuilt
  from a string-keyed record with three lookups per field).
* Every function-typed `_smelt_tmp_` local is initialized with a freshly allocated default
  callback (`predeclared_local_needs_default`'s blanket rule, ~337 sites in this crate, ~8 in
  `#dispatch` alone). Relying on the existing definite-assignment analysis instead would remove
  those allocations, but the rule has no recorded rationale and dropping it risks E0381 in
  corpora that cannot be compiled locally here; left for a change with corpus CI behind it.
* `IntoSmeltUnknown` for a nominal record still renders callable fields as `null` (the generic
  impl has no ABI for them); §0's fix routes the `...rest` copy around it, but any other caller
  of that impl loses callables the same way.
* Pre-existing divergence found while writing `request_url_serialization`: the `url` crate keeps
  `^` in a path where Node (WHATWG) encodes `%5E`. Not part of this change; left out of the fixture.
* Two Smelt limitations hit while writing fixtures (worked around in the fixture, not fixed): a
  field write through a module-level class instance is rejected ("write through a mutable global
  is only lowered for Record/Map globals"), and a user function named `main` collides with the
  generated `fn main`.
