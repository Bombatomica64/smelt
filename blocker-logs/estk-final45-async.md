# es-toolkit final 45 — group: ASYNC / PROMISES / TIMERS / ABORT

Read-only investigation (no cargo). Every claim below was verified against the
*current* generated crate at `third_party/es-toolkit/dist-smelt/src/` and the
current transpiler sources, not against earlier blocker logs.

Four assigned tests, four **distinct** roots. None of them is a timer-scheduler
or clock defect: `crates/smelt-runtime/src/clock.rs`, the virtual-time timer
queue (`smelt_set_timeout`/`smelt_sleep_ms`), `SmeltPromise`/`SmeltFuture` and
`smelt_abort_signal_fire` all behave correctly in each case. Three of the four
roots are *frontend-ts lowering* defects that drop or fabricate a value before
the runtime ever sees it.

---

## 1. `attempt should return the result of the promise`

**Spec** `third_party/es-toolkit/src/util/attempt.spec.ts:18-22`

```ts
const [error, result] = attempt(async () => 1);
expect(error).toBeNull();
expect(await result).toBe(1);   // line 21 — JS answers 1
```

`attempt` returns `[null, func()]`; `func()` is an async arrow, so `result` holds
a `Promise<number>` and `await result` is `1`.

### Wrong generated Rust

`dist-smelt/src/attempt_spec.rs`, `test_attempt_should_return_the_result_of_the_promise`:

```rust
_smelt_tmp_7 = SmeltUnknown::Number(1.0 as f64);
_smelt_tmp_8 = !(matches!(_smelt_tmp_7, SmeltUnknown::Null | SmeltUnknown::Undefined));
if _smelt_tmp_8 {
    return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::String("expect(...).toBe(...) failed: expect(await result).toBe(1) ...".into())));
```

`_smelt_tmp_7` is the **expected** `1`, erased to `SmeltUnknown`. The **actual**
(`await result`) is not in the emitted code at all: it was lowered to a `null`
constant, so `unknown_binary_text` took its `rhs_is_erased && lhs_is_none` arm

```rust
// crates/smelt-codegen-rust/src/emitter/binary_ops.rs:213
} else if rhs_is_erased && lhs_is_none {
    format!("matches!({}, {})", self.operand_text(rhs)?, nullish_pattern(lhs))
```

and rendered `1 == undefined`. Negated for `toBe` -> `!(false)` -> always throws.
The `await` never runs: the `SmeltUnknown::Promise` sitting in `result[1]` is
never driven, and `SmeltPromise::smelt_await` is never called.

### Root cause

`crates/smelt-frontend-ts/src/lowering/new_expr.rs`,
`Expression::AwaitExpression` arm (~line 2388). When `future_inner_type(awaited_ty)`
is `None` **and** there is no contextual `type_hint`, the whole `await X` is
replaced by a null literal and the operand is discarded:

```rust
let ty = self.ctx.krate.types.intern(Type::Unknown);
return Ok(body.push_expr(Expr {
    kind: ExprKind::Literal(Literal::None),   // <-- `await X` becomes `null`, X dropped
    ty,
    span: self.span(await_expr.span.start, await_expr.span.end),
}));
```

Here `result` is `Option<SmeltUnknown>` (element 1 of an erased array
destructure), i.e. `Type::Optional(Unknown)` — not a `Type::Future`. The
immediately preceding branch already handles exactly this shape correctly, but
only when a `type_hint` exists:

```rust
if let Some(resolved_ty) = type_hint && self.erased_or_union_surface(awaited_ty) { ... Await(TypeAssert -> Future(resolved_ty)) ... }
```

`expect(...)`'s actual argument is lowered by
`matchers.rs::expect_matcher_call` via `self.argument(actual_arg, body)` — no
hint — so the hint is absent and the drop branch wins.
`erased_or_union_surface` does accept `Optional(Unknown)`
(`lowering/ty/assignability.rs:305-315`), so only the missing hint blocks it.

Layer: **frontend-ts lowering** (one function). Nothing downstream is wrong: the
emitter can already lower `await <erased>` — `coercion.rs`
`checked_extraction`'s `Some(Type::Future(output))` arm (line ~2599) emits

```rust
{ let smelt_erased_future = (…).into_smelt_unknown(); SmeltFuture::from_future(Box::pin(async move { let smelt_awaited = smelt_await_flatten(smelt_erased_future).await?; … })) }
```

and the prelude helper `smelt_await_flatten` (`lib.rs:2366`) is already a
promise-chain drain with an identity pass-through for non-promises — exactly JS
`await` on a non-thenable.

### Shared root?

No other assigned test. Nothing else in `failures.txt` obviously shares it (this
is the only `expect(await …)` over an erased value in the 45).

### Verdict: (a) general defect, fixable — size S

Rule change in `new_expr.rs`'s `AwaitExpression` arm, replacing the drop branch:

1. If `self.erased_or_union_surface(awaited_ty)`: use
   `type_hint.unwrap_or_else(|| intern(Type::Unknown))` as the resolved type and
   take the existing `Await(TypeAssert -> Future(resolved))` path. (Today the
   branch is gated on `type_hint.is_some()`; drop that gate.)
2. Otherwise, if the awaited type is a concrete non-future type, `await v` in JS
   *is* `v`: return `awaited` unchanged.
3. Keep no path that discards the operand. If neither 1 nor 2 applies, raise a
   blocker instead of silently emitting `null`.

Regression test shape: a new
`crates/smelt-codegen-rust/tests/await_erased_value_runtime.rs` in the style of
`vitest_async_matcher_runtime.rs`:

```ts
function pair(): [null, unknown] { return [null, (async () => 1)()]; }
const [, result] = pair();
expect(await result).toBe(1);          // must pass
const [, plain] = [null, 7] as [null, unknown];
expect(await plain).toBe(7);           // await of a non-thenable is identity
```

Assert on the *generated text* too (no `Literal::None` substituted for the
actual) so the silent-drop cannot come back.

---

## 2. `withTimeout lifts the time limit when the signal is aborted, resolving with the run result`

**Spec** `third_party/es-toolkit/src/promise/withTimeout.spec.ts:27-41`

```ts
const controller = new AbortController();
setTimeout(() => controller.abort(), 25);      // disarm before the 50ms deadline
const result = await withTimeout(async () => { await delay(100); return 'foo'; }, 50, { signal: controller.signal });
expect(result).toEqual('foo');
```

Observed failure: the test throws
`DOMException { message: "The operation was timed out", __smelt_class: "TimeoutError" }`
— the timeout fired even though the signal aborted at 25ms.

### Wrong generated Rust

`dist-smelt/src/timeout.rs`, inside `timeout_419`, the `abortHandler` closure:

```rust
let _smelt_tmp_6 = ::std::rc::Rc::new(|| {
    let _smelt_tmp_0: SmeltRecord<String, SmeltUnknown> = SmeltRecord::from([]);
    let _smelt_tmp_1: SmeltUnknown = SmeltUnknown::Object(SmeltObject::from_unknown_record(_smelt_tmp_0.clone()));
    let _smelt_tmp_2: () = smelt_clear_timeout(_smelt_tmp_1.clone());
    SmeltUnknown::Undefined
});
let abort_handler = _smelt_tmp_6.clone();
```

Source is `const abortHandler = () => { clearTimeout(timeoutId); };`.
`clearTimeout(timeoutId)` was emitted as **`smelt_clear_timeout(<fresh empty
object>)`** — the reference to `timeoutId` was replaced by an empty object
literal, and the closure captures nothing (contrast the sibling closure
`_smelt_tmp_9`, which does carry a `let signal = signal.clone();` capture
prelude).

The handle ABI is a number:

```rust
fn smelt_set_timeout(...) -> SmeltUnknown { ...; SmeltUnknown::Number(id as f64) }
fn smelt_clear_timeout<T: IntoSmeltUnknown>(handle: T) {
    let SmeltUnknown::Number(id) = handle.into_smelt_unknown() else { return; };   // <-- empty object: early return
```

so the abort handler is a **no-op**. `controller.abort()` at 25ms *does* run the
listener (`smelt_abort_method(..., "addEventListener")` registered
`abort_handler` on `__smelt_abort_listeners`, and `smelt_abort_signal_fire`
invokes it) — it just clears nothing. The 50ms timer then fires and
`reject(new TimeoutError())` wins the `Promise.race`.

### Root cause

`timeoutId` is a `const` declared **after** the closure that reads it (legal in
JS: the closure only runs later). Smelt lowers statements in source order, so at
closure-lowering time `timeoutId` is unbound in `self.scope`, and
`lowering/expr/references.rs::identifier_expression` falls through the unbound
chain to

```rust
// crates/smelt-frontend-ts/src/lowering/expr/references.rs:~305
if self.source_contains_forward_callable(name) {
    let ty = self.ctx.krate.types.intern(Type::Unknown);
    return self.module_global_expression(name, ty, start, end, body);
}
```

`source_contains_forward_callable` is a **text** heuristic
(`stdlib/call_dispatch.rs:1301`): it returns true for any name for which the
module source contains `const <name> =` — which `const timeoutId = setTimeout(...)`
satisfies. `module_global_expression` with `Type::Unknown` then fabricates the
value (`references.rs:1410-1426`):

```rust
Some(Type::Class { .. } | Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
    ... ExprKind::DictLit(Vec::new()) ... UnknownCast { value, target: ty }
```

i.e. the empty object we see. This is the *same* failure mode a comment two
hundred lines above already documents for `this`: "it used to be listed with the
ambient globals below, which routed it through `module_global_expression` and,
for `Type::Unknown`, fabricated an EMPTY OBJECT LITERAL". The forward-reference
route was never fixed the same way.

The existing forward-reference machinery covers only arrow-valued consts:
`matchers.rs::predeclare_local_arrow_callbacks` (line 2731) predeclares a local
for a `const` whose initializer is an arrow function, or whose initializer
mentions its own name (`initializer_needs_deferred_self_binding`). A `const`
whose initializer is an ordinary *call* (`setTimeout(...)`) and which is read by
an *earlier* closure is not predeclared.

Layer: **frontend-ts lowering** —
`predeclare_local_arrow_callbacks` (too narrow) +
`identifier_expression`'s `source_contains_forward_callable` fallback (fabricates
a value for a function-local name).

### Shared root?

Very likely a family, though the other two members happen not to fail: the same
`const id = setTimeout(...)` / earlier-closure-reads-it shape appears in
`debounce`/`throttle`. It does *not* share a root with test 3 below (also
debounce/abort-flavoured) — that one is `vi.spyOn`.

### Verdict: (a) general defect, fixable — size M

Two coupled rule changes:

1. **Generalise the predeclare pass** (`matchers.rs::predeclare_local_arrow_callbacks`
   -> `predeclare_forward_referenced_locals`): before lowering a statement list,
   for every `const`/`let` declarator in it, reserve a `LocalDecl` when any
   *earlier* statement in the same list references that name from inside a
   function/arrow body. The name-collection walk already exists
   (`callbacks/body_lowering.rs::collect_statement_capture_names` /
   `collect_expression_capture_names`), so this is a scope pass, not new
   analysis. The declaration lowering then assigns into the reserved local, and
   the earlier closure captures it through the ordinary `CaptureMode::ByMut`
   shared-cell path (`classify.rs::collect_callback_captures`) — the same
   `smelt_capture_*` `Rc<RefCell<..>>` capture already visible in
   `debounce_1.rs` as `(*smelt_capture_cancel.borrow())`.
2. **Stop the fabrication**: `source_contains_forward_callable` must not fire for
   a name declared as a local later in the current function body. With (1) in
   place that case no longer reaches the fallback; if any case still does, it
   must raise `SmeltError::for_unresolved_name`, never a `DictLit` default. A
   forward reference is a binding, not a host global.

Regression test shape: extend
`crates/smelt-codegen-rust/tests/abort_signal_runtime.rs` (or a new
`forward_const_capture_runtime.rs`):

```ts
function arm(signal: AbortSignal, ms: number): void {
  const onAbort = () => clearTimeout(id);          // reads `id` declared below
  const id = setTimeout(() => { fired = true; }, ms);
  signal.addEventListener('abort', onAbort, { once: true });
}
```
Abort before the deadline, advance the clock past it, assert `fired === false`.
Add a generated-text assertion that no `SmeltRecord::from([])` is passed to
`smelt_clear_timeout`.

---

## 3. `debounce should not add multiple abort event listeners`

**Spec** `third_party/es-toolkit/src/function/debounce.spec.ts:155-175`

```ts
const addEventListenerSpy = vi.spyOn(signal, 'addEventListener');   // line 160
...
const listenerCount = addEventListenerSpy.mock.calls.filter(([event]) => event === 'abort').length;
expect(listenerCount).toBe(1);                                       // line 173 — JS answers 1
```

### Wrong generated Rust

`dist-smelt/src/debounce_spec.rs`:

```rust
let add_event_listener_spy: SmeltUnknown = SmeltUnknown::Null;
...
_smelt_tmp_25 = match add_event_listener_spy.clone() { SmeltUnknown::Object(map) => smelt_get_object_field(&map, "mock"), _ => SmeltUnknown::Undefined }.clone();
_smelt_tmp_26 = /* erased_to_list of `_smelt_tmp_25.calls` */ ... SmeltUnknown::Null | SmeltUnknown::Undefined => SmeltList::new(Vec::new()) ...;
_smelt_tmp_29 = _smelt_tmp_28.len() as f64;   // 0
```

`mock.calls` reads off `Null`, coerces to the empty list, so `listenerCount == 0`.

### Root cause

`crates/smelt-frontend-ts/src/lowering/stdlib.rs::vitest_spy_on_call` (line 357)
lowers the target and then **throws it away**:

```rust
let _target = self.argument(target, body)?;
...
Ok(Some(self.vitest_mock_handle_expr(span, body, is_date_timezone_offset_spy)))
```

and `vitest_mock_handle_expr` (line 514) returns `ExprKind::Literal(Literal::None)`
typed `Unknown` for everything except the one `Date.getTimezoneOffset` class
marker. So every `vi.spyOn` is an inert `null` placeholder; nothing is installed
on the target and nothing is recorded.

Layer: **frontend-ts lowering** (`stdlib.rs`), plus the runtime mock prelude
(`smelt-codegen-rust/src/lib.rs`, `smelt_vitest_mock_new`).

### Re-verification of the earlier "out of scope" ruling

`blocker-logs/estk-remaining-triage.md:661-682` judged this out of scope on two
grounds. Ground 1 (`vi.spyOn` is an inert placeholder) is **still true**. Ground
2 — "a host `AbortSignal` whose `addEventListener` is an interceptable member at
all … today there is no property for a spy to wrap" — is **wrong against the
current emitter**. The generated member read already prefers a real field and
only synthesizes the host method when the key is absent
(`dist-smelt/src/debounce_1.rs:234`, and identically `timeout.rs`):

```rust
SmeltUnknown::Object(map) if (map.contains_key("__smelt_abortcontroller") || map.contains_key("__smelt_abortsignal")) && !map.contains_key("addEventListener")
    => smelt_abort_method(map.clone(), "addEventListener"),
SmeltUnknown::Object(map) => match map.get("addEventListener") { ... }
```

So a spy that inserts a callable under the `"addEventListener"` key on the signal
object **would** be found and called by library code. The signal itself is a
plain `SmeltObject` marker record
(`{ __smelt_abortsignal, aborted, __smelt_abort_listeners }`) whose fields are
freely insertable. Nothing about the host seam has to be regated.

The recording half also already exists: `smelt_vitest_mock_new(Some(impl))`
builds a real object with `__smelt_call`, `mockRestore`/`mockClear`/… and a
`"mock"` field read (`main.rs:4074`) that materialises
`{ calls: [[...], ...], results: [...] }` from live state.

### Verdict: (a) general defect, fixable — size M (was previously reported as scope; that ruling no longer holds)

General rule (no method-name or library special-casing): `vi.spyOn(target, name)`
lowers to a runtime boundary adapter, e.g. a new prelude helper
`smelt_vitest_spy_on(target: SmeltUnknown, name: &str) -> SmeltUnknown`, which:

1. resolves the *current* value of `target[name]` through the same
   field-else-synthesized-host-method rule the member emitter uses (factor that
   `contains_key(name) ? field : smelt_abort_method(...)` decision out of
   `emitter/optional_access.rs`/the member-read emitter into one prelude helper
   `smelt_host_method(object, name)` so both call sites share it — this is what
   keeps it general);
2. builds `smelt_vitest_mock_new(Some(original))` so the default outcome
   forwards to the original implementation (that is what makes the listener
   actually register, so `debounce`'s single registration still happens);
3. `object.insert(name, mock.__smelt_call)` — and links the mock object so
   `mock.calls` resolves;
4. records `(object, name, original)` in a thread-local restore table that
   `mockRestore` / `vi.restoreAllMocks` replays.

Frontend change: `vitest_spy_on_call` stops discarding `_target` and emits an
`ExprKind::AsyncOp`-style runtime call (or a new `ExprKind::VitestSpyOn { target,
name }`, matching the existing `VitestMockCalledTimes` family) typed `Unknown`.
`vitest_mock_handle_expr`'s `Literal::None` placeholder should then be reachable
only for genuinely unmodellable targets.

Note the *currently passing* `toHaveBeenCalledTimes`/`toHaveBeenCalledWith`
matchers "pass vacuously" on a non-mock actual by design
(`matchers.rs:200-206`); once spies are real, that vacuous arm should be revisited
in the same change or a follow-up, or some suites will start asserting for real.

Regression test shape: new
`crates/smelt-codegen-rust/tests/vitest_spy_on_runtime.rs` —

```ts
const controller = new AbortController();
const spy = vi.spyOn(controller.signal, 'addEventListener');
controller.signal.addEventListener('abort', () => {});
expect(spy.mock.calls.length).toBe(1);
expect(spy.mock.calls[0][0]).toBe('abort');
controller.abort();          // forwarding still fired the real registration
```
plus a plain-object case (`vi.spyOn(obj, 'method')` where `obj.method` is a
user function) to prove the rule is not abort-specific.

---

## 4. `reduceAsync without initial value returns undefined for empty array without initial value`

**Spec** `third_party/es-toolkit/src/array/reduceAsync.spec.ts:101-109`

```ts
const arr: number[] = [];
const reducer = vi.fn(async (acc: number, n: number) => acc + n);
const result = await reduceAsync(arr, reducer);
expect(result).toBeUndefined();   // line 106 — JS answers undefined
```

### Wrong generated Rust

`dist-smelt/src/reduceAsync_spec.rs`:

```rust
let result: f64;                       // <-- f64, so it can never be undefined
...
let _smelt_tmp_6 = { let smelt_source_future = SmeltFuture::from_future(Box::pin(reduce_async(...))); SmeltFuture::from_future(Box::pin(async move {
    let smelt_future_value = smelt_source_future.await?;
    Ok::<_, _>(match smelt_future_value.clone() { SmeltUnknown::Number(value) => value, ...,
        SmeltUnknown::Null | SmeltUnknown::Undefined | ... => f64::NAN })   // <-- undefined becomes NaN
})) };
_smelt_tmp_7 = _smelt_tmp_6.await?;
result = _smelt_tmp_7;
_smelt_tmp_8 = !(false);               // <-- `f64 !== undefined` folded to a constant
if _smelt_tmp_8 { /* always throws */ }
```

The library function is right: `reduce_async(...) -> Result<SmeltUnknown, _>`
returns `SmeltUnknown::Undefined` for the empty array
(`initial_value = Some(array.get(0)…unwrap_or(SmeltUnknown::Undefined))`, then
`accumulator = initial_value.expect(...)`). The **call site** destroys it twice:

1. `emitter/coercion.rs::extract_value_text`'s `Some(Type::Float)` arm applies a
   JS `ToNumber` (`Null | Undefined | … => f64::NAN`) — a *coercion* where the
   source performs no conversion at all (the declared type is erased at runtime).
2. `emitter/binary_ops.rs::unknown_binary_text` then folds
   `result !== undefined` with the arm

   ```rust
   } else if lhs_is_none || rhs_is_none {
       "false".to_owned()
   ```
   giving `!(false)` — the assertion can never pass.

Fold (2) is *correct given the types*; the lie is the `f64`.

### Root cause

`reduceAsync`'s overloads both declare `Promise<T>` / `Promise<U>` (non-optional),
and the implementation obtains `undefined` only through
`initialValue = array[0] as unknown as U`. The frontend instantiates the selected
overload's return with `T = number`
(`lowering/stdlib/call_dispatch.rs::instantiate_overload_signature`, line 3495 —
or the single-signature path at line 931 for non-overloaded generics), while the
lowered callee's Rust return is the erased `SmeltUnknown`. MIR therefore types
the call as `Future(Float)` and the emitter inserts the lossy extraction. So a
declared concrete type is asserted over a runtime value that can be nullish, and
the extraction *manufactures* a value rather than admitting absence.

Layer: **frontend-ts type instantiation** (`call_dispatch.rs`) +
**codegen-rust emitter** (`coercion.rs::extract_value_text`).

### Shared root?

Yes — same emitter symptom (`!(false)`), and a closely related family in
`failures.txt`:

* `maxBy if array is empty return undefined` and `minBy …` — `maxBy_spec.rs` has
  `let result: Person;` and the very same `_smelt_tmp_5 = !(false);`. Here the
  callee is properly generic and honest:
  `max_by_120<T: …>(items: SmeltList<T>, get_value: &F0) -> Option<T>`, and the
  call site **collapses the `Option`** with
  `.map_or(Default::default(), |value| …)` because the *selected overload* is
  `maxBy<T>(items: readonly [T, ...T[]], getValue): T` rather than
  `maxBy<T>(items: readonly T[], getValue): T | undefined` — a plain `Person[]`
  argument was matched against a non-empty-tuple parameter. That is an
  **overload-applicability** defect for `readonly [T, ...T[]]` (a separate,
  smaller fix), but it lands on the same shared safety-net rule below.
* `at should return undefined for non integer indices` and
  `zip … [3, undefined]` are plausible further members (not verified here; other
  groups own them).

### Verdict: (a) general defect, fixable — size M (safety-net rule) / L (full honest boundary)

Two rules, in priority order:

1. **A checked extraction may never manufacture a value for a nullish payload.**
   In `emitter/coercion.rs::extract_value_text` (and its callers), extraction
   from an erased value to a primitive target must be nullish-preserving: the
   extraction site's type becomes `Optional(T)` (`Option<f64>`), with
   `Null | Undefined => None`. JS `ToNumber`/`ToString`/`ToBoolean` coercion stays
   only where the *source* actually coerces (`Number(x)`, `+x`, arithmetic,
   template interpolation) — those already route through the explicit cast paths.
   With that, `result: Option<f64>` is `None`, and `expect(result).toBeUndefined()`
   lowers through the existing
   `binary_ops.rs::optional_erased_singleton_equality_text` and passes; the
   sibling `returns first element for single-element array` compares
   `Some(42.0) == 42.0` through the already-present `Some(x) == y` arm
   (`binary_ops.rs:380`). This keeps concrete types (no new `SmeltUnknown`) and
   is the honest boundary CLAUDE.md asks for.
2. **A callee's presence information wins over a declared non-optional type.**
   When the resolved callee's lowered return is `Optional(T)` (maxBy) or erased
   (reduceAsync) but the instantiated signature says `T`, the call expression
   keeps `Optional(T)`; it must not be collapsed with
   `map_or(Default::default(), …)`. There is precedent for exactly this guard in
   `call_dispatch.rs:934-943`, where a generic return is deliberately *not*
   concretized because the callee "lowers to the fully-erased
   `SmeltErasedFunction`, whose calls yield `unknown` regardless of the declared
   payload … concretizing its return from the argument types would desync the
   value's runtime shape from the concrete type its call sites would then
   expect." Generalise that from erased *function* returns to erased/optional
   *value* returns.

   Separately (smaller, independent): make overload applicability respect
   `readonly [T, ...T[]]` minimum arity so `maxBy(people: Person[], …)` selects
   the `T | undefined` overload.

Regression test shape: new
`crates/smelt-codegen-rust/tests/declared_type_nullish_boundary_runtime.rs`:

```ts
function pick(xs: number[]): number { return xs[0] as unknown as number; }   // declared number, may be undefined
expect(pick([])).toBeUndefined();
expect(pick([7])).toBe(7);
```
and an `Option`-returning-callee case mirroring maxBy, asserting the generated
text contains no `map_or(Default::default()` on that call and no `=> f64::NAN`
arm for the nullish variants.

---

## Summary

| test | root family | verdict | size |
| --- | --- | --- | --- |
| `attempt should return the result of the promise` | `await <non-Future, erased>` is lowered to a `null` literal and the operand dropped — `new_expr.rs` `AwaitExpression` fallback | (a) general defect, fixable | S |
| `withTimeout lifts the time limit when the signal is aborted…` | forward reference to a later `const` in the same function body fabricates `{}` via `source_contains_forward_callable` -> `module_global_expression`, so `clearTimeout(timeoutId)` becomes `smelt_clear_timeout({})` | (a) general defect, fixable | M |
| `debounce should not add multiple abort event listeners` | `vi.spyOn` discards its target and returns an inert `null` handle (`stdlib.rs::vitest_spy_on_call`); the member-read seam and the mock runtime already support a real spy, so the earlier "out of scope" ruling does not hold | (a) general defect, fixable (feature-shaped) | M |
| `reduceAsync without initial value returns undefined…` | declared concrete type asserted over an erased/optional runtime value: lossy `extract_value_text` `ToNumber` (`Undefined => NaN`) + `unknown_binary_text` folding `f64 !== undefined` to `false`. Shares family with `maxBy`/`minBy` (there via an overload-applicability defect on `readonly [T, ...T[]]`) | (a) general defect, fixable | M (safety-net) / L (full) |

No test in this group is out of scope. Nothing here requires DOM, cross-realm
`node:vm`, Node `Buffer`, or global monkey-patching; the `vi.spyOn` case needs a
mutable property on a Smelt-modelled marker object, which the emitter already
reads.
