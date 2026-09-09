# The throwing rvalue: why a fallible stdlib MEMBER becomes a call terminator

Round 28, item 3. Owner: standards-tier implementer. Date: 2026-09-09.

## The problem, stated as the generated code sees it

`DataView.prototype.getInt16(offset)` throws a `RangeError` when the window it
addresses is not wholly inside the view. JavaScript makes that error
**catchable**, so this program has to print `RangeError`, not abort:

```ts
const view = new DataView(new ArrayBuffer(4));
try {
  view.getInt16(6);
} catch (error) {
  console.log(error instanceof RangeError, (error as RangeError).name);
}
```

Before this round the accessor lowered to `Rvalue::DataViewAccess`, assigned by
a `Statement::Assign`. That is the shape the whole problem comes down to:

* MIR's exception edges live on **terminators**. `Terminator::Call` and
  `Terminator::Await` each carry an `unwind: Option<BlockId>`; a
  `Statement::Assign` carries nothing.
* The `catch` block is reachable only through such an edge. With no edge, the
  handler block has no predecessor, is dropped as dead, and the "throw" becomes
  whatever the infallible helper answered — for the accessor, a silent `0.0`.

So a fallible operation spelled as an rvalue cannot reach a source `catch` at
all. It is not a matter of emitting a `?` in the right place: the control-flow
graph has nowhere for the failure to go.

`JSON.parse` already had this shape and already had the answer: it is a
`Callee::Builtin(BuiltinFn::JsonParse)` behind a `Terminator::Call`, whose
`unwind` is `self.current_exception_handler()`. The URI decoders and the base64
pair followed it. The accessor is the first fallible stdlib **member** — every
earlier one was a free function — and it takes the same route.

## The two designs, and why the call terminator wins

**A. Hoist fallible rvalues into calls in a MIR pass.** Keep
`Rvalue::DataViewAccess`, then run a pass that finds every fallible rvalue and
rewrites the block, splitting it and re-terminating with a `Call`.

Rejected. The pass has to re-derive, per rvalue kind, which operand is the
receiver and which are the arguments, in order to build the call's argument
list — information the lowering site already has and the pass would have to
duplicate. It also has to know the enclosing `try`, but the exception handler
stack is a **lowering-time** structure (`current_exception_handler`), gone by
the time a pass over finished MIR runs; reconstructing it means recovering
scope nesting from the CFG. And every consumer of MIR between lowering and the
pass would see a shape that is a lie about the program's control flow. The pass
buys nothing: the only thing it saves is naming the builtin, which is one
enum variant.

**B. Lower the accessor straight to a call terminator.** `Rvalue::DataViewAccess`
is deleted; `BuiltinFn::DataViewAccess { write, element }` replaces it, and HIR's
`ExprKind::DataViewAccess` carries the same two fields.

Chosen. The unwind edge is available exactly where it is needed — at lowering,
where the handler stack is live — and the fallibility is a property of the
builtin (`BuiltinFn::is_fallible`), asked of it rather than spelled per site.
That question already existed and had one caller that named `JSON.parse`
directly; making it a method on the builtin is what fixed the general gap where
every fallible builtin added after `JSON.parse` failed to mark its enclosing
function as throwing.

## What the accessor's identity is, and where it comes from

The eighteen accessors (`getInt8` .. `setFloat64`) differ in exactly two ways:
the **direction** and the **element type** (its width and signedness). The
source member NAME encodes both and nothing else does, so the registry rule
`smelt_stdlib::data_view_accessor` reads it **once**, at the frontend dispatch,
and the resolved pair rides on the node from there:

```rust
DataViewAccess { write: bool, element: smelt_stdlib::TypedArrayElement }
```

Two consequences worth stating, because both were the alternative:

* The element type is `smelt-stdlib`'s own, named rather than copied. HIR now
  depends on `smelt-stdlib` (a leaf crate, serde only, so no cycle and no build
  weight) so that the accessor rule, the emitted kind enum and the eleven view
  classes keep **one** element table. A second table is the bug this avoids.
* Carrying the member string instead would make the emitter re-parse it, which
  is a second place the mapping exists, and would let a spelling JavaScript does
  not define (`getUint8Clamped`) reach the emitter. With the pair resolved at the
  dispatch, it cannot: the same registry rule is what allowed the call to lower.

## Where the bound is checked, and where the throw is made

The prelude's `SmeltDataView` keeps its infallible `get`/`set` and gains a
`get_checked`/`set_checked` pair over one `in_bounds` expression, so the checked
and unchecked halves cannot disagree about where the window ends. The **throw**
is not in the prelude type: it is in the generated adapters
(`smelt_data_view_get_throwing` / `smelt_data_view_set_throwing`), which answer
`Result<_, Box<dyn Error>>` and turn `None` / `false` into
`smelt_throw(error_payload_record_expr("RangeError", ..))` — the same error
channel and the same branded payload as the decoders and the base64 pair.

Two things follow from that split, and both are deliberate:

1. **The infallible members stay infallible.** A typed array's element read, a
   `subarray`, a `fill` — none of them throws, and putting a `Result` on the
   view type would put a `?` in every generated signature that touches one.
2. **The message is Node's, verbatim** (`Offset is outside the bounds of the
   DataView`), because `catch (e) { e.message }` is ordinary JavaScript and a
   program can read it.

The adapters are gated by `stdlib::needs_data_view_access_runtime`, which scans
**terminators**, not rvalues — the rvalue-based dependency scan cannot see a
call, which is the same reason `needs_uri_decode_runtime` exists. A program
whose only byte-family use is `view.getInt16(0)` therefore still gets the whole
family emitted, and a program with no accessor pays for neither.

## The siblings this opens

`TypedArray.prototype.set` (a source longer than the destination is a
`RangeError`), `ArrayBuffer.prototype.resize` and the `Atomics.*` accessors are
the same shape: a fallible stdlib member whose failure a program can catch. Each
is one `BuiltinFn` variant and one adapter, with no new machinery. `DataView` is
the one that pays for the road.

## What the fixture measured about erasure, and the rule it produced

`88_data_view_range_error` is the first fixture whose whole subject is a caught
error, so it was also the first to put fifteen avoidable-erasure lines into the
zero-erasure examples corpus. Both causes are worth recording, because one was a
fixture bug and the other was a misclassification.

Three lines were the fixture's own doing: `(error as RangeError).name` stores the
caught value in a second erased local before reading it. TypeScript types a
`catch` binding `unknown` by its own rule, so the cast buys nothing that an
`instanceof` narrowing does not — the narrowing form is used throughout instead,
and the three locals are gone by construction. (`76_base64_globals` made exactly
the same change for `DOMException` in the same round.)

The other twelve were the guard itself. `error instanceof RangeError` lowers to

```rust
matches!(error.clone(), SmeltUnknown::Object(value)
    if matches!(value.get("__smelt_error"),
        Some(SmeltUnknown::String(class)) if &*class == "RangeError" || ..))
```

— a read of the thrown value's brand slot to recover its class at run time. That
is the policy's "values inspected through runtime narrowing" case, and it is the
*only* way a program can identify a thrown error: the payload arrives on the
exception channel whose ABI is already classified as a boundary
(`smelt_throw` / `smelt_thrown_value` carry a branded record and a bare string
down one `Result<_, Box<dyn Error>>` on a run-time branch), and no concrete
type, generated union arm, or scoped generic can carry that. So the fix is in
`classify_line`, with the marker shaped like the `Error.cause` rule — the brand
probe inside the guard and nothing else, proven by
`error_brand_instanceof_guard_is_a_boundary`, which pins that an erased local
and an erased record beside the guard both stay avoidable. The `instanceof`
against a HOST brand (`DOMException`, `DataView`, `Request`) is the same edge and
got the same treatment in the same round.

With both changes the fixture is at avoidable 0, which is what the corpus's hard
invariant requires of it.
