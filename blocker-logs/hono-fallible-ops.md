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
