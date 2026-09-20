# CI on `main` after 7899b194 — root causes and fixes

Branch: `worktree-agent-a267f07db32f63c7c`, cut from `main` and merged up to `95a38789`.
Failing runs addressed: `35516329735` (CI) and `35516329694` (Runtime tiers), head `7899b194`.

Every fix is a general rule; nothing keys off a fixture's spelling. One test expectation was
changed, and only after diffing it against Node 22.22 (item 6).

## 1. `corpus_emitted_rust_compiles` — `[py_try_lambda_classbody]`, E0425 `smelt_panic_message`

**Root cause.** `emit_throwing_call_terminator` renders `catch_unwind` plus the panic-recovery
helpers for every MIR `Call` that carries an exception handler — *including one whose callee is
infallible*, because a closure coerced to a non-throwing callback parameter can only report a
`throw` by panicking, so the `catch` still has to recover it. The prelude gate
(`stdlib::needs_panic_route`) asked a different question: "can any SIGNATURE in this crate throw?"
The corpus program has a `try` around a call to an infallible `apply`, and nothing in it can throw,
so the helper was called and never defined.

**Fix.** `needs_panic_route` now also answers true when any lowered body holds a `Call` with an
exception handler — the emit site's own condition, so one decision drives both ends of the route.
`mir_catches_a_call` in `crates/smelt-codegen-rust/src/stdlib.rs`.

**Regression tests.** `thrown_tests::a_catch_around_an_infallible_call_defines_the_panic_helpers_it_calls`
(the Python corpus program) and `..._a_typescript_catch_defines_the_panic_helpers_it_calls`, both
asserting the crate-level invariant "a panic-route helper call implies its definition". Both fail
with the new clause disabled (verified).

## 2. `callback_generics_fixtures_compile` — four recorded failures now compile

Not a regression: rounds 29-33 fixed the shapes. `concrete_and_generic_callbacks_two_sinks`,
`second_type_param_pinned_by_key_callback`, `string_length_in_callback_only` and
`two_call_sites_pin_differently` are removed from `EXPECTED_FIXTURE_FAILURES`, so they are ordinary
green fixtures now and a future regression in them fails the tier.

Four recorded error COUNTS also drifted and were re-snapshotted (`generic_class_method_callback`
208 -> 276, `generic_class_method_and_free_maker` 208 -> 282, `generic_class_two_methods_callback`
4 -> 2, `source_class_named_box_with_callback_sink` 27 -> 43). Per the file's own documented rule
(`ExpectedFailure::errors`) count drift is reported, not asserted, so it never failed the tier; the
CI log's "records N, observes 0" lines are that same non-blocking drift channel.

## 3. Runtime tier `standards` / `web_crypto_runtime` — two defects, all four cases red

**3a. E0308 x ~50: a typed-array element read was reported as erased.** `place_ty`'s `Place::Index`
arm fell through to the erased fallback for a typed-array view, while
`typed_array::typed_array_index_read_text` rendered `view.get(i).unwrap_or(0.0)` — an `f64`. A
caller therefore coerced an already-concrete value as if it were erased, and `Number(view[i])` ran
the `SmeltUnknown` numeric-conversion match over an `f64` scrutinee. The whole class came from one
four-line `hex()` helper. The read's text and its type are now decided by the same rule, as they
already were for a concrete `.length` and for a `String` field read.

**3b. E0425 `smelt_next_object_id`.** The object-identity prelude gate listed its demanders by hand
and did not name `SmeltPrimSet`, whose `new` mints an id, so a program whose only identity-bearing
container was a primitive-keyed `Set` called a counter the crate never declared. The list is now a
named `mints_object_ids` predicate and also covers the event emitter, the `node:http` server and
the request/response/body runtimes. (Same family as item 1: a demand tracked separately from the
emission that creates it.)

**Regression tests.** `part_7_tests::a_typed_array_element_read_is_a_concrete_number`,
`part_7_tests::a_primitive_set_program_defines_the_object_id_counter`, plus the
`set_collection_emission` snapshot, which now carries the counter it calls.

## 4. Runtime tier `async` / `promise_value_fidelity_runtime` — "union guard selected an excluded member"

**Root cause.** `options?.shouldRetry ?? DEFAULT_SHOULD_RETRY` interned the union
`((attempt: number) => boolean) | (() => boolean)`. TypeScript types `a ?? b` as
`NonNullable<typeof a> | typeof b`, and that union REDUCES to the left arm here, because a callback
may ignore trailing arguments — `() => boolean` inhabits `(attempt: number) => boolean`. Keeping
the union produced a generated `SmeltUnion` whose call site knew only the declared arm, so
selecting the fallback hit `unreachable!` at run time.

**Fix.** The `??` lowering collapses to the left arm whenever the fallback is assignable to it or
widens to it by arity, and the fallback enters through the ordinary assertion adapter; the Rust
backend already renders the arity-widening closure (`function_shape_adapter_text`).
`function_value_widens_to` states the widening rule next to `function_arity_assignable`, which
answers the different question of whether the value can be used AS IS.

## 5. Runtime tier `async` / `timer_runtime` — E0277, `?` on a `SmeltFuture`

**Root cause.** An async function reports even a synchronous throw as a REJECTED future, so
`function_value_return_type_text` deliberately puts no `Result` around a future-returning function
value. Five call sites read `may_throw` directly and rendered `?` anyway, so `const p = boom()` on
a throwing async arrow emitted `(boom)()?` against `SmeltFuture<f64>`.

**Fix.** `function_value_call_is_fallible` — the mirror of that rendering — is now what every one of
those sites asks, so the `?` and the emitted type cannot disagree.

## 6. Runtime tier `server` / `node_http_runtime` — the expectation was wrong

`header_writes_replace_case_insensitively_and_write_head_merges` recorded `"missing":null` for
`JSON.stringify({ missing: res.getHeader('x-absent') })`. `getHeader` answers `undefined` for a
header never set, and `JSON.stringify` OMITS an undefined-valued property. Node 22.22.2, run
against a real `node:http` server, prints `{"type":"text/csv","kept":"yes","status":202}` — exactly
what the generated program printed. The generated output was right; the expectation was corrected,
with the Node check recorded in a comment beside it.

## 7. Runtime tier `server` / `request_runtime` — `bodyUsed` after `new Request(erasedInput)`

Pre-existing, and already written up in `blocker-logs/hono-round30-emitter.md` ("Found, not
fixed"): the `Request` adapters REBUILT a request from its erased record, so `new Request(input)`
on an erased `string | Request` disturbed the rebuilt copy and the original's `bodyUsed` stayed
false where Node makes it true at once.

**Fix.** `host_value_erasure` already states the rule — "Identity, not reconstruction": erasing a
value and narrowing it back is the SAME object, which is why every other host runtime type retains
its live value in `SMELT_HOST_ORIGINS` under the erased record's object id. `Request` and
`Response` hand-wrote their adapters before that shared emitter existed and never joined it. They
now register on erasure and restore on recovery, falling back to the structural rebuild only for a
record that did not come from an erasure in this thread. The body handle and its `bodyUsed` cell
are `Rc`-shared, so the restored request is the original in every observable way. No new gate: the
registry is already emitted whenever `needs_headers_runtime` holds, which both types imply.

## 8. CI job `hono-advisory`

The job is documented and configured as advisory (`continue-on-error` at the job level), but its
`cargo check` step on the generated Hono crate exited non-zero and marked the job failed. The crate
has three tracked errors (`blocker-logs/hono-current.md`) and the step exists to report them, so it
carries `continue-on-error: true` like the probe steps above it. YAML only.

## Gate numbers (all from a freshly built binary)

| gate | result |
| --- | --- |
| `cargo test -p smelt-codegen-rust` (lib + integration) | 1079 lib, all integration suites green |
| `compile_corpus -- --ignored` | 2/2 (both tiers) |
| `web_crypto_runtime` | 4/4 |
| `promise_value_fidelity_runtime` | 7/7 |
| `timer_runtime` | 8/8 |
| `request_runtime` | 4/4 |
| `node_http_runtime` | 5/5 |
| rest of `standards` / `async` / `server` shards | green (fetch_types 9, text_codec 5, blob 6, form_data 6, host_erasure 4, abort_signal 9, abort_reason 4, loop_await 4, generator 14, truthiness_and_await 7, response 5, fetch_init 8, fetch_response 1, event_emitter 6, uri_decode_throw 6) |
| spot-checked neighbours | typed_array 8, throwing_callback 8, erased_call_dispatch 2, closure_try_catch 3, thrown_payload 6 |
| `cargo test -p smelt-frontend-ts --no-default-features` | 1093 |
| `cargo test -p smelt-transpiler` | 57 + 15 + 17 + 27 + 3, 0 failed |
| `cargo clippy --all-targets` | no errors; no new findings in touched files |
| examples invariant | avoidable **0** (+0 vs baseline); prelude +991 and boundary +0 — the prelude drift is pre-existing in the committed baseline (this branch's golden diff adds and removes zero `SmeltUnknown` occurrences) and never blocks |
| es-toolkit ratchet | prelude 2619 (+0), boundary 40297 (+0), avoidable 31645 (+0) |
| remeda | 1789 passed / 0 failed |
| radash | 84 passed / 0 failed |
