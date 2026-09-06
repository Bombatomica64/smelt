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

