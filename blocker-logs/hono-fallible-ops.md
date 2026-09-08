# Fallible stdlib operations and absent globals: one throwing mechanism

Owner: Hono implementer. Round 2, item 2. Date: 2026-09-06.

Decision from the coordinator: a call to a global the non-DOM profile declares
absent lowers to a thrown `ReferenceError`, catchable by the surrounding `try`;
and it lands together with the fallible-rvalue family so all of them use one
mechanism.

---

## 1. First: the round-1 claim was wrong, and the code comment says so

Round 1 reported that `decodeURI`'s `.expect(...)` was "the same single root
cause as the existing `JSON.parse` emission". **That is not true, and I am
correcting it here rather than carrying it forward.**

`JSON.parse` does *not* lack an unwind edge. It was fixed during the es-toolkit
campaign and is the reference implementation for exactly this problem:

- `smelt_mir::BuiltinFn::JsonParse` exists as a **builtin callee rather than an
  rvalue** and its own doc comment states the reason: "A builtin rather than an
  rvalue because it is *fallible*: malformed text throws a catchable
  `SyntaxError`. Only `Terminator::Call` and `Terminator::Await` carry an
  `unwind` edge, so a fallible operation has to be a call to reach an enclosing
  `try`."
- `crates/smelt-mir/src/lower/expr.rs` lowers `ExprKind::JsonParse` to
  `Terminator::Call { callee: Callee::Builtin(BuiltinFn::JsonParse), …,
  unwind: self.current_exception_handler() }`.

So there is no shared gap. There is a **correct pattern** (`JSON.parse`) and
three or four operations that never adopted it.

`Rvalue::JsonParse` still exists in `types.rs` and is still handled in
`format.rs`, `opt/mod.rs`, `validate/operands.rs` and
`emitter/call_runtime.rs`, but **nothing constructs it** — the only `Rvalue::
JsonParse {` occurrences are those match arms. It is the dead pre-fix form. §6
covers whether to remove it.

The stale claim is also written into a code comment at
`crates/smelt-codegen-rust/src/emitter/strings.rs` (`uri_transcode_text`),
which tells the next reader the hole "is one general gap shared with
`JSON.parse`, to be closed once for both". That comment must be corrected in
the same commit as the fix; leaving it would send someone to fix a thing that
is already right.

## 2. What actually still aborts

| operation | current MIR form | throws in JS | today in generated Rust |
| --- | --- | --- | --- |
| `JSON.parse` | `Callee::Builtin(JsonParse)` + unwind | `SyntaxError` | correct, catchable |
| `decodeURI` | `Rvalue::UriTranscode { op: Decode }` | `URIError` | `.expect("URIError: URI malformed")` — aborts |
| `decodeURIComponent` | `Rvalue::UriTranscode { op: DecodeComponent }` | `URIError` | same, aborts |
| `atob` | not lowered at all | `InvalidCharacterError` | blocker |
| `btoa` | not lowered at all | `InvalidCharacterError` (lone surrogate) | blocker |
| a call to an absent global | not lowered at all | `ReferenceError` | blocker |

`encodeURI` / `encodeURIComponent` are **infallible** for well-formed UTF-16
input and `UriTranscodeOp::is_fallible()` already answers that; they keep the
plain rvalue and get no unwind edge. That distinction is the reason the fix is
per-op and not "make `UriTranscode` a call".

Why it matters for Hono concretely: `src/utils/url.ts` has

```ts
const tryDecode = (str: string, decoder: (str: string) => string): string => {
  try { return decoder(str) } catch { /* fall back to a partial decode */ }
}
```

With `.expect(...)` the `catch` is not merely unreachable in practice — the
handler block has no predecessor, so MIR drops it, and the fallback Hono
deliberately wrote is *gone from the generated crate*. That is a silent
behaviour change, which is the class of defect this campaign exists to find.

## 3. The mechanism, stated once

Every fallible operation becomes a `BuiltinFn` variant reached through
`Terminator::Call` with `unwind: self.current_exception_handler()`. Nothing
else changes: the throwing-function propagation pass
(`lower/passes/throwing.rs`) already keys off `Callee::Builtin(...)` calls, and
`intern_fallible_builtin_return_types` already exists to intern a builtin's own
return type — it is currently hard-coded to the single `JsonParse` case and
becomes a loop over the fallible set.

New variants:

```rust
pub enum BuiltinFn {
    ConsoleLog, ConsoleWrite, ConsoleErrorWrite,
    JsonParse,
    /// `decodeURI` / `decodeURIComponent`: throw `URIError` on malformed input.
    UriDecode(smelt_hir::UriTranscodeOp),
    /// `atob`: throws `InvalidCharacterError` on non-base64 input.
    Base64Decode,
    /// A call to a global the profile declares absent: always throws
    /// `ReferenceError: X is not defined`.
    AbsentGlobal,
}
```

`btoa` is **infallible** for the input Smelt can represent: a Rust `String` is
well-formed UTF-8, so the lone-surrogate case that makes `btoa` throw in
JavaScript cannot arise. It stays a plain rvalue. Recording that here because
"btoa throws in JS" is true and still does not justify an unwind edge — the
question is whether the *Rust* operation can fail, and it cannot.

### `AbsentGlobal` is a terminator, not a diagnostic

A call to `addEventListener` in the non-DOM profile is not a compile error and
not an erased no-op. It is a program that *runs* and throws, exactly as Node
does:

```
ReferenceError: addEventListener is not defined
```

Hono's `hono-base.ts` calls it under its own `@ts-ignore`, in a position where
the surrounding code tolerates the throw. Erasing it to a no-op would be a
false green (the handler silently never registers); refusing to lower it keeps
a blocker for a program that is correct. Throwing is the only answer that
matches the runtime.

This also means `NON_DOM_ABSENT_GLOBALS` gains `addEventListener` (and
`removeEventListener`, `dispatchEvent`, which are the same surface — a set with
one of three entries would be an accident waiting to happen). The set already
drives `global_member_presence`, so `"addEventListener" in globalThis` starts
folding to `false` in the same change, which is correct and is what makes the
feature-probe path and the call path agree.

## 4. Runtime side

`atob` needs base64 decoding. Per the coordinator: use the `base64` crate,
pay-for-use — the dependency is emitted only when the generated crate actually
calls it, the same gating `smelt_stdlib::host_module_dependencies` and
`stdlib.rs`'s `any_rvalue_needs` already do for `regex`.

Runtime helpers, in `crates/smelt-runtime/src/`:

- `smelt_atob(&str) -> Result<String, SmeltThrow>` — base64 decode, then the
  latin-1 → `String` mapping `atob` specifies (each byte becomes one code
  point, *not* UTF-8 decoding; getting this wrong is the usual `atob` bug).
- `smelt_btoa(&str) -> String` — infallible per above.
- the two decoders already exist (`smelt_decode_uri`,
  `smelt_decode_uri_component`) and already return `Result`; only the *emission*
  changes, from `.expect(...)` to the call terminator's error edge.

## 5. Acceptance

The coordinator named the acceptance test: **`tryDecode`'s catch becomes
reachable.** Concretely, in a runtime tier fixture:

1. a `try { decodeURIComponent(bad) } catch { fallback }` returns the fallback
   rather than aborting — one fixture per op (`decodeURI`,
   `decodeURIComponent`, `atob`, absent global);
2. the same shape with no enclosing `try` propagates out of the function, which
   is what `throwing.rs` is for, and the caller can catch it;
3. `encodeURI` emits **no** unwind edge (a negative test, so a later change
   cannot quietly make every transcode fallible);
4. a MIR-level assertion that the `catch` block has a predecessor, because that
   is the actual defect — a runtime assertion alone would pass on an
   `.expect()` that happens not to fire.

## 6. Open, and deliberately not decided here

`Rvalue::JsonParse` and its four match arms are dead code. Removing them is
correct but touches `format.rs` snapshot output and `validate/operands.rs`,
which is unrelated churn in a commit about throwing edges. Recorded as a
follow-up rather than bundled: the risk is that a future reader adds a fifth
handler for it and believes the rvalue form is live.

---

## 7. Round 4 implementation plan, narrowed to what is actually landing

The ruling is the two URI decoders only: `atob`/`btoa` are no longer
Hono-demanded (they sat inside the now-excluded jwt and client surfaces, per
`hono-scope.md`), so adding the `base64` dependency has no caller to justify it
this round. §3's `Base64Decode` and `AbsentGlobal` variants are deferred; the
absent-global half already landed in round 2 as a throwing closure rather than
a builtin, which is a different and simpler shape than §3 sketched.

So exactly one variant is added:

```rust
/// `decodeURI` / `decodeURIComponent`: throw `URIError` on malformed input.
///
/// A builtin rather than an rvalue for the same reason as `JsonParse`: only
/// `Terminator::Call` and `Terminator::Await` carry an `unwind` edge.
UriDecode(smelt_hir::UriTranscodeOp),
```

`UriTranscodeOp::is_fallible()` already answers which ops belong here
(`Decode` / `DecodeComponent`), so the split is not a new judgement — the
frontend has recorded it all along and only the MIR lowering ignored it.
`encodeURI` / `encodeURIComponent` keep `Rvalue::UriTranscode` and get **no**
unwind edge; that is asserted by a negative test so a later change cannot
quietly make every transcode fallible.

### The four edits

1. `BuiltinFn::UriDecode(op)` in `smelt-mir/src/types.rs`.
2. `lower/expr.rs`'s `ExprKind::UriTranscode` arm branches on
   `op.is_fallible()`: fallible ops build `Terminator::Call { callee:
   Callee::Builtin(BuiltinFn::UriDecode(op)), unwind:
   self.current_exception_handler(), .. }` — a copy of the `JsonParse` arm
   directly above it — and infallible ops keep `assign_temp`.
3. `intern_fallible_builtin_return_types` stops being hard-coded to
   `JsonParse`. A decoder returns `String`, not `Unknown`, so the function
   becomes a scan over the fallible builtins actually called, interning each
   one's own return type.
4. The backend emits the call: `emitter/call*.rs` gains the `UriDecode` callee,
   producing `smelt_decode_uri(..)?`-shaped code at the terminator instead of
   `strings.rs`'s `.expect("URIError: URI malformed")`. The `.expect` emission
   is deleted, and with it the stale-comment correction from round 2.

### Acceptance, restated concretely

`tryDecode`'s catch is the point of the exercise:

```ts
const tryDecode = (str: string, decoder: (s: string) => string): string => {
  try { return decoder(str) } catch { return str }
}
```

Today the `catch` block has no predecessor and MIR drops it, so Hono's
deliberate fallback is *absent from the generated crate*. The fixture asserts
the fallback is taken for malformed input — a runtime assertion, because a
type-level one cannot tell a dropped handler from a live one.

---

## 8. What landed, and the one shape that still aborts

Landed: `decodeURI` / `decodeURIComponent` lower to
`Terminator::Call { Callee::Builtin(BuiltinFn::UriDecode(op)), unwind }`, two
generated adapters convert the runtime decoders' `Option<String>` into a thrown
`URIError` through the same payload ABI a source-level `throw` uses, and the
`.expect("URIError: URI malformed")` emission is gone — the rvalue path now
*reports* a fallible op reaching it rather than re-emitting the abort.

Verified by running a generated crate: a malformed input takes the `catch`
fallback instead of aborting. That is the acceptance criterion, and it holds for
a decoder **called directly inside the `try`**.

Two things running it taught that reading the code did not:

**The adapters have to force the `needs_unknown` prelude region on.** A thrown
value is a JavaScript error object, carried as a `SmeltUnknown`, and the payload
ABI lives inside that region. `JSON.parse` gets there for free because its own
return type *is* `Type::Unknown`; a decoder returns `String`, so its adapter was
emitted into a block the program never entered and the generated crate failed
with E0425. `needs_unknown_type` now names throwing decoders explicitly — which
is a true statement about any throwing program, not a patch for this one.

**A decoder reached through a callback VALUE still aborts, and marking it
throwing makes things worse.** Hono's actual shape is

```ts
tryDecode(str, decodeURIComponent)   // decoder passed as a value
```

The value form lowers to a closure. Marking that closure `may_throw: true` is
the obvious fix and is **wrong**: `may_throw` is part of the function *type*,
TypeScript has no way to spell "this callback throws", so the declared parameter
type `(value: string) => string` wins, and the coercion adapter inserts an
unwrap against a Rust closure that is not throwing — E0599, a compile break
instead of a runtime abort. That change was made, run, and reverted; the
reverted site carries the reason so the next reader does not repeat it.

Closing it needs `may_throw` **inference through callback parameter types**: a
parameter whose argument can throw has to widen, and every caller of that
parameter has to see the widened type. That is a type-system change, not an
emission change, and it is the same machinery a `throws` annotation would need.
Recorded here rather than attempted, because the half-measure is a compile
error.

So Hono's `tryDecode` is **not** yet fixed, while `try { decodeURI(x) } catch`
is. The remaining gap is one named, tested-around design issue rather than an
unexplained abort.

---

## 9. Round 5: the premise was wrong, and what the real defect is

Ruling received: infer a callback parameter's fallibility whole-crate in HIR, on
the grounds (from my own §8) that Hono's `tryDecode(str, decodeURI)` aborts.

**I checked that before implementing, and it does not abort. §8 was wrong.**
Correcting it first, because the design's justification rested on it.

### 9.1 What actually happens today

`tryDecode(str, decodeURIComponent)` with malformed input **takes the catch**.
Run on the round-4 merged head, three of three assertions pass:

```ts
const tryDecode = (str: string, decoder: (value: string) => string): string => {
  try { return decoder(str); } catch { return str; }
};
const viaValue = (value: string): string => tryDecode(value, decodeURIComponent);
// viaValue('%E0%A4%A') === '%E0%A4%A'   ✓
// viaValue('a%20b')    === 'a b'        ✓
// an infallible callback through the same parameter also works ✓
```

The mechanism is panic-as-exception, and both halves are already in the tree:

* the adapter closure wrapping the decoder emits
  `smelt_decode_uri_component_throwing(..).unwrap_or_else(|error| panic!("{}", error))`,
  because its own type says `may_throw: false` so
  `body_can_propagate_error()` is false;
* the `try` in `tryDecode` emits
  `std::panic::catch_unwind(AssertUnwindSafe(|| (decoder)(str)))`, so it catches
  that panic and runs the handler.

Separately, `lower/expr.rs`'s `ClosureCall` arm already gives an indirect call
the unwind-carrying terminator form whenever a handler is active, regardless of
the callee's declared `may_throw` — its comment says exactly why. So there was
never a missing unwind edge on the call side either.

**Why §8 got it wrong:** in round 4 I only ever ran this shape *with* my
`may_throw` change applied, which broke compilation (E0599). I reverted the
change and reported the shape as broken without re-running it. The revert left
a working configuration. This is the same failure mode as the round-2 radash
report — a conclusion drawn from a configuration I had not actually executed —
and the lesson is the one already recorded: re-run after reverting, not just
after changing.

### 9.2 The defect that IS real

Panic-as-exception loses the error's identity. Same fixture, asking for
`error.name`:

| route | `error.name` for malformed input |
| --- | --- |
| direct — `try { decodeURIComponent(x) } catch (e)` | `URIError` ✓ |
| callback value — `tryDecode(x, decodeURIComponent)` | **not `URIError`** ✗ |

The direct route uses the `Result` path and carries the structured record. The
callback route panics with `panic!("{}", error)` — a *formatted string* — and
the catch emission only ever downcasts one type:

```rust
let __smelt_error = if let Some(message) = __smelt_panic.downcast_ref::<String>() {
    message.clone()
} else if let Some(message) = __smelt_panic.downcast_ref::<&'static str>() {
    (*message).to_owned()
} else { "JavaScript exception".to_owned() };
```

so a `catch (error)` binding gets a string where JavaScript gives a `URIError`.
`error.name`, `error instanceof URIError`, and `error.message` are all wrong.
That is a silent wrong value, which is the class this campaign exists to find.

Two further consequences of the same mechanism, neither of which a test would
catch:

* **`panic = "abort"` breaks it.** The generated `Cargo.toml` sets no `panic`
  strategy, so the `unwind` default applies and it works today — but a consumer
  adding `panic = "abort"` for size turns a catchable `URIError` into a process
  abort, with nothing to warn them.
* **stderr noise.** No panic hook is installed, so every caught error prints a
  panic message plus a backtrace note. For a router decoding untrusted path
  segments that is per-request output on ordinary input.

### 9.3 Two ways to fix it, and they are not the same size

**(a) Carry the payload through the panic.** Panic with the structured
`SmeltUnknown` (or the `SmeltThrown` wrapper) instead of a formatted string, and
have the catch emission downcast to that first, falling back to the existing
string arms. This fixes identity for *every* panic-routed throw, not only the
decoders, and it is contained to the throw and catch emission sites. It does not
address `panic = "abort"` or the stderr noise — those are inherent to routing
control flow through panics.

**(b) The inference from the ruling.** Infer the parameter's fallibility so the
argument closure can be `may_throw` without breaking the coercion, and the call
through the parameter propagates a `Result` instead of panicking. This removes
the panic route entirely for statically-resolvable cases, so it fixes identity,
the abort strategy, and the noise together. It is a whole-crate HIR fixpoint
plus MIR-lowering and emission changes, and it must not regress the callback-
dense corpora.

They compose: (a) is the correct floor for the cases (b) cannot resolve
statically — an erased callback out of a data structure will always take the
panic route, and its identity should survive that.

**Recommendation: (a) now, (b) as its own round.** (a) fixes the observed wrong
value at its cause with a small, testable change; (b) is the larger design and
deserves the full gate run against es-toolkit / remeda / radash rather than
being rushed alongside a correction. I have not started either — I stopped to
report, because the ruling's stated justification is void and I did not want to
spend the round building on it.

### 9.4 If (b) proceeds, the design still stands

§9's earlier draft of the rule survives the correction, with one simplification:
the inference does **not** need to create unwind edges (§9.1 shows the call side
already has them when a handler is active). It exists to make a throwing
argument type-compatible with its parameter, so the argument need not panic.

* the join rule, the three conservative cases (unresolvable argument, escaping
  function, inherited fallibility) and the `while changed` fixpoint as ruled;
* recorded in a side table keyed by `(ItemId, param index)`, never in
  `Type::Function` — widening the interned type is the E0599 of round 4;
* HIR rather than the emitter, because the unwind edge is attached during
  HIR->MIR lowering and a handler with no predecessor is already gone by the
  time the emitter runs (this is why the ownership fixpoint can live in the
  emitter and this one cannot);
* acceptance as ruled, plus **`error.name === 'URIError'` through the callback
  route**, which is the assertion that actually fails today and the one that
  proves the panic route is gone rather than merely working.

## 10. Round 6: (a) landed — the panic route keeps the throw's class

Ruling: do (a) now, defer (b). This is what (a) is.

### 10.1 The shape of the fix

The panic route had two ends and both were lossy at once.

**The throw end.** Twenty-six emit sites across seven emitter modules rendered
`.unwrap_or_else(|error| panic!("{}", error))` — a *formatted string*. They now
render `.unwrap_or_else(|error| smelt_panic_throw(error))`, and the adapter
panics with a payload instead of text:

```rust
struct SmeltPanic { class: String, message: String }
fn smelt_panic_throw(error: Box<dyn Error>) -> ! {
    smelt_install_panic_hook();
    ::std::panic::panic_any(smelt_panic_payload(&*error))
}
```

The payload is a class plus a message and **not** the thrown `SmeltUnknown`,
because `panic_any` requires `Any + Send` and a `SmeltUnknown` holds `Rc`
handles. Those are the two parts a `catch` binding observes through the
exception-payload record, so the record can be rebuilt on the other side. Custom
fields on a thrown class instance still do not cross the unwind; that is a
property of the route, and it is one of the reasons (b) exists.

`smelt_panic_payload` projects the class the same way JavaScript reports
`error.name`: the `__smelt_error` brand `new <ErrorClass>(m)` writes, else the
`name` property a user error class carries.

**The catch end.** Both `Err(__smelt_panic)` arms in
`emitter/control_flow.rs` downcast only `String` and `&'static str`, then built
the erased record with `panic_payload_record_expr`, whose class is the literal
`"Error"`. They now call `smelt_panic_message` / `smelt_panic_error_value`, which
try `SmeltPanic` first and recover the real class. `error_payload_record_expr`
grew a sibling taking the class as a rendered *expression* rather than a literal,
which is what a run-time class needs.

**The hook.** `smelt_install_panic_hook` runs once, from inside
`smelt_panic_throw`, so it is installed exactly when the route is first taken and
nowhere else — no generated `main` change, and it works in a library crate under
`cargo test`. It suppresses the report **only** for a `SmeltPanic` payload and
delegates every other panic to the previous hook, so a genuine panic is as loud
as it was.

### 10.2 Pay-for-use

`stdlib::needs_panic_route` gates the whole family on "something in this crate's
signatures admits a throw" (a function item, a closure, or an interned
`Type::Function` with `may_throw`). A crate with no throwing signature emits none
of it — verified by the four emission snapshots that were *reverted* after the
gate went in, and by the two that legitimately keep it.

The structured `smelt_panic_payload` and `smelt_panic_error_value` name
`SmeltUnknown`, so they are emitted inside the prelude's `needs_unknown` region
and only when the route is needed. A crate with no erased values also has no
structured throw payloads — its throw sites carry plain message strings — so the
class is `Error` there by construction, which is the second body of
`smelt_panic_payload`.

### 10.3 `panic = "abort"`

The route is unwind-dependent by construction: `catch_unwind` cannot catch an
abort. `deps::cargo_toml` now says so where the profile is built, and
`no_profile_sets_panic_abort_while_the_panic_route_exists` asserts that the
emitted manifest mentions no panic strategy under any allocator or
release-profile combination. The word `panic` appearing anywhere in the manifest
fails the test, which is deliberately blunt: there is no spelling of a panic
strategy that this route survives.

### 10.4 Acceptance

`tests/uri_decode_throw_runtime.rs`, six fixtures, all executing real generated
crates:

| fixture | route | asserts |
| --- | --- | --- |
| `the_thrown_uri_error_is_a_real_catchable_error_value` | direct (`Result`) | `error.name === 'URIError'` |
| `the_callback_value_route_keeps_the_uri_error_identity` | callback value (panic) | `error.name === 'URIError'`, for both decoders, plus a well-formed input still decoding |
| `the_panic_route_keeps_a_user_error_class_identity` | callback value (panic) | `new TypeError('bad x')` arrives as `TypeError` / `bad x` |

The third is the one that shows the fix is general rather than per-builtin: it
throws a `TypeError` from hand-written source through a declared non-throwing
callback parameter, which is the same route with none of the URI machinery in it.

### 10.5 (b) is deferred — the justification, restated

(b) is the callback `may_throw` inference (§9.4). Round 5's justification for it
was *reachability* — that Hono's `tryDecode` catch was dead — and §9.1 showed
that justification is void: the catch is reachable and always was. **(b) is not
deferred because the corrected premise made it unnecessary; it is deferred
because its remaining justification is a different, narrower three-part one, and
that is worth its own round rather than a rushed landing.** The three parts:

1. **Identity, beyond a class and a message.** (a) carries what is `Send`. A
   thrown class instance's custom fields, its `cause`, and its prototype
   identity (`error instanceof MyError`) still do not cross the unwind. (b)
   removes the unwind for the statically resolvable cases, so the whole payload
   survives.
2. **Abort strategy.** The route makes `panic = "abort"` a silent
   behaviour change for the generated crate, which (a) can only *pin against*
   (§10.3), not remove. A consumer who wants that profile for size cannot have
   it while any throw is panic-routed. (b) removes the route where the callee is
   known.
3. **Noise.** (a)'s hook silences the report, which is the right floor, but a
   silenced panic is still an unwind per caught error — allocation, hook lookup,
   `Box<dyn Any>` — on ordinary input for a router decoding untrusted path
   segments. A `Result` return costs none of that.

None of the three is reachability. All three are real, and all three are bounded
by the same fact: (b) can only help where the argument is statically resolvable,
so **(a) remains the correct floor** for an erased callback pulled out of a data
structure, which will always take the panic route. They compose; they are not
alternatives.
