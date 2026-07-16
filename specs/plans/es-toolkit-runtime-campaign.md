# Campaign Plan: es-toolkit Generated Suite Toward Green

*(Fable planning pass, 2026-07-12. Read-only; hypotheses verified against generated code + runtime prelude.)*

## 1. Hang Hypothesis Analysis (verified against code)

### H1 — Virtual-clock starvation: `smelt_sleep_ms(0)` can never advance time to a pending timer (HIGH confidence — the `attemptAsync` hang)
- Promise-executor lowering emits a spin-wait future: `dist-smelt/src/delay_1.rs:67` — `loop { if let Some(result) = smelt_promise_result.borrow_mut().take() { break result; } tokio::task::yield_now().await; smelt_sleep_ms(0.0).await; }`. The resolve callback fires only from a `smelt_set_timeout(..., ms)` timer (delay_1.rs:57).
- `smelt_sleep_ms` (emitted from `crates/smelt-codegen-rust/src/lib.rs:1546-1577`) computes `target_ms = now + delay` and only fires timers with `due_ms <= target_ms`. With delay=0, a timer due at now+100 is never selected; virtual time never moves.
- `attemptAsync_spec.rs:148-220` awaits `delay_76(100.0, ...)` — infinite spin. The four non-timer attemptAsync tests pass/fail fast, consistent with only some async tests hanging.
- **Fix shape (general):** Node-style run-until-idle — when the spin loop makes no progress and the result cell is empty, advance `SMELT_TIMER_NOW_MS` to the earliest pending timer (or make `smelt_sleep_ms(0)` do so when timers are pending and no promise task is runnable). Lives in the runtime prelude timer section + possibly the executor loop emission in `emitter/call.rs`.
- **Experiment:** `timeout 30 cargo test <delayed attemptAsync test> -- --test-threads=1` vs the four non-delay tests — only the delayed one should hang; hand-patch the generated sleep in a scratch copy to confirm.

### H2 — Control-flow lowering bug: `continue` re-enters the OUTER loop instead of re-checking a compound inner `while` condition (HIGH confidence — the `combinations` hang; NOT async)
- `combinations_spec.rs` tests are plain sync `#[test]`. In `dist-smelt/src/combinations.rs`, the inner compound-condition `while (i >= 0 && indices[i] === i+n-r) i--` lowers to `continue` statements targeting the OUTER `loop` (line 83): at lines 102-111, the decrement is followed by `continue`, restarting the outer loop, pushing a duplicate tuple and resetting `i_2` — infinite loop with growing `result` (explains the SIGKILL/memory).
- Signature of the same bug elsewhere: dead mirrored branch `_smelt_tmp_N = false; if _smelt_tmp_N { ... }` (combinations.rs:130-163). Grep dist-smelt for that shape to find further miscompiles (likely wrong-answer failures too).
- **Fix shape:** in `emitter/control_flow.rs` (or MIR loop lowering), a nested `while` with a short-circuit condition must get its own loop header/label so `continue` re-evaluates the inner condition.
- **Experiment:** `timeout 10` + `/usr/bin/time -v` on a 2-length combinations test — expect timeout + large max-RSS.

### H3 — `SmeltPromise::smelt_await` yield-spin with no driver (MEDIUM)
`lib.rs:1018-1030`: if `state` is None and `future` is None (clone raced, or future handed to `smelt_spawn_promise_task`, lib.rs:1436-1441), the loop never calls `smelt_drain_promise_tasks`; under a current-thread runtime `yield_now` spins forever. Fix direction: spin loop also drains promise tasks / advances the clock (same driver fix as H1).

### H4 — `RefMut` held across `.await` in `SmeltPromise::smelt_await` (LOW — panic, not hang)
`lib.rs:1019-1022`: pre-2024-edition temporary-scope rules keep the `RefMut` alive across `future.await`. Bind `let taken = self.future.borrow_mut().take();` before awaiting regardless; check dist-smelt Cargo.toml edition.

### H5 — `smelt_drain_promise_tasks` 64-round bound (LOW; causes failures, not hangs)
`lib.rs:1447-1467` gives up after 64 rounds — chains of >64 dependent yields silently leave detached tasks unfinished.

Ranking: H1 ≈ H2 (effectively confirmed) > H3 > H4 > H5.

## 2. Failure-Family Triage Strategy

**Step 0 — survivable baseline**: `cargo run --bin smelt -- rust-test-report --build-manifest third_party/es-toolkit/Smelt.toml --cargo-manifest third_party/es-toolkit/dist-smelt/Cargo.toml --full --suppress-warnings --output blocker-logs/es-toolkit-baseline.md`. For hang enumeration, wrap once as `timeout 300 cargo test ... -- --test-threads=1` (first hanging test identifiable by last-printed name); parallel otherwise (specs call `smelt_reset_timers()` and timer state is thread-local).

**Step 1 — bucket the 89 by assertion signature**, attack order:
1. **Hangs (H1+H2)** — must land first; they hide results and block full-suite gating.
2. **Compound-condition/loop lowering miscompiles (H2 family)** — grep the dead-branch signature; likely also wrong-answer toEqual failures.
3. **debounce/throttle Default-initialized callable objects** — known: `SmeltFuture<T>: Default` + `smelt_default_callback` no-ops make timer-driven callables silently do nothing. `--focus debounce --focus throttle`.
4. **vi.fn() spies without call tracking** — `toHaveBeenCalled*` lower to constant checks: `attemptAsync_spec.rs:134,209` contains `_smelt_tmp_10 = !(false); if _smelt_tmp_10 { return Err(...) }` — unconditionally failing assertions. Grep specs for `!(false)` / `!(true)` — cheap, high-yield bucket.
5. **toEqual on erased values** — verify `==` on `SmeltUnknown::Array` ignores the identity `id` (`SmeltArray::with_id` comparisons in specs); if not, one fix clears many toEqual failures.

## 3. Round Structure (max 2 Opus coding agents/round; orchestrator writes no feature code)

**Round 1 (hang elimination):** Agent A: H1 runtime clock driver (lib.rs prelude + emitter/call.rs executor loop), regression fixture = `await delay(100)` under a promise executor. Agent B: H2 inner-while continue targeting in control-flow lowering, fixture = the `while (i >= 0 && cond) i--` shape. Disjoint (runtime prelude vs MIR control flow).

**Rounds 2+:** one bucket per agent from the ordered list, two per round, each scoped by `--focus`, accumulating `--guard` filters for every fixed family + `--baseline-report` against the prior round.

**Gates per round:** workspace check/clippy/test; `rust-test-report --full --baseline-report` no-new-failures + guards green; cross-language stdout runs; smelt-unknown-report no avoidable-erasure rise; bounded-timeout hang gate on the full generated suite; fixes general (no per-function special cases); mtime-preserving writes intact; one commit per family with report path.

**Stopping criterion:** zero hangs AND every remaining failure is either an upstream-semantics question needing a user decision (never silently reject) or a <3-test family with no general transpiler defect. Target: 56 → ~90+ passing after Round 1 alone; if a round yields <5 net new passes twice in a row, pause and re-triage.

Key files: `crates/smelt-codegen-rust/src/lib.rs:1546-1577` (sleep/clock), `:1436-1468` (spawn/drain), `:1018-1030` (promise await spin), `:1063-1113` (SmeltFuture); `emitter/call.rs`, `emitter/control_flow.rs`; generated evidence `delay_1.rs:57,67`, `combinations.rs:83-163`, `attemptAsync_spec.rs:134,164,209`.
