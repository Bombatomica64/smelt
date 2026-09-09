# H17 — a class-field arrow that touches `this.#private`

Round 9, Hono implementer. Site: `third_party/hono/src/context.ts:533`.

```ts
status = (status: StatusCode): void => {
  this.#status = status
}
```

## The attribution in the dispatch was wrong, and here is the proof

The round-9 brief attributed the `context.ts` blocker

```text
field access is only lowered for Record<string, T>, class, and interface values
for now (receiver: Float, field: status)
```

to line 651, `const status = typeof arg === 'number' ? arg : (arg?.status ?? this.#status)`,
and asked for two rules: `typeof` narrowing on a ternary's else branch, and a
field read over a union whose every arm declares the field. **Neither is
missing.** A module-scope reproduction of exactly that line —

```ts
function ternaryElse(arg?: Code | OrInit): number {
  return typeof arg === 'number' ? arg : (arg?.status ?? 7)
}
```

with `Code` a numeric-literal union, `Init<T extends Code = Code>` a generic
interface and `OrInit = Init<T> | Res` a generic alias to a union — lowers with
no diagnostic, and MIR reads `%3 = copy %0?.field5` off
`Optional<Float | Res | Init<Float>>`: the else branch keeps the WHOLE union
(no narrowing needed to make the read work) and `class_field_type`'s
`Type::Union` arm already joins the arms that declare the field. Line 615's
`typeof arg === 'object' && arg.headers` lowers too. Removing line 651 from a
copy of `context.ts` did NOT remove the blocker, which is what finally ruled the
site out.

The real site was found by making the diagnostic panic under an env var and
reading the backtrace: `class_field_type` <- `private_field_member` <-
`assignment_target_expr` <- ... <- `arrow_closure_body_expr` <-
`emit_class_field_initializers`. That is `this.#status = status` inside the
class-field ARROW at line 533, not a read at 651. `Float` was the type of the
arrow's first PARAMETER.

## What was actually wrong — three defects in one chain

Minimal reproduction (no Hono types at all):

```ts
class C {
  #status: number | undefined
  setStatus = (value: number): void => { this.#status = value }
  read(): number { return this.#status ?? 0 }
}
```

### 1. `this` was not captured (frontend)

A class-field arrow's captures are discovered by walking its body for names
bound outside it (`collect_expression_capture_names` and the two assignment-
target collectors in `lowering/callbacks/body_lowering.rs`). Those walkers knew
`StaticMemberExpression` (`this.x`) and `ComputedMemberExpression` (`this[k]`)
but had `_ => {}` for `PrivateFieldExpression`. So `this.#status = value`
captured NOTHING; inside the closure `this` resolved to whatever the enclosing
scope offered — the arrow's own first parameter.

Consequences, both general:

* a numeric-ish first parameter made it a HARD BLOCKER
  (`receiver: Float, field: status`) — this is the `context.ts` blocker;
* a `string` first parameter compiled, because `class_field_type` answers
  `Unknown` for a string receiver, and the write was emitted as
  `let _ = SmeltUnknown::Number(..)` — **the write was silently dropped**. Node
  prints `4` for the fixture, the generated crate printed `0`.

Fixed by treating a private-field member expression exactly as a static one in
all four walk positions: `Expression::PrivateFieldExpression`,
`ChainElement::PrivateFieldExpression` (`this?.#x`),
`SimpleAssignmentTarget::PrivateFieldExpression` (`this.#count++`) and
`AssignmentTarget::PrivateFieldExpression` (`this.#status = v`).

### 2. The class was not lifted to reference semantics (codegen classify)

With `this` captured, the write reached the instance the CONSTRUCTOR built and
was lost again: a by-value class struct means the capture cell holds the
instance and the constructor returns a snapshot of it, so an arrow called later
mutates the cell while the caller holds the copy.

`classify::collect_self_capture_triggers` lifted a class whose METHOD captures
`this`, but a class-field arrow is initialized in the CONSTRUCTOR, where `this`
is a user binding rather than a parameter, so it never fired. The general rule
that landed: a constructor whose instance ESCAPES into a closure needs the
shared handle, because the closure outlives the constructor. Construction itself
stays excluded — `collect_field_write_triggers` still ignores a constructor's
own direct writes to the instance it is building; escaping is not construction.

### 3. A callable field of a reference class (codegen emit)

Lifting `C` exposed two emitter gaps, both general to reference classes:

* `place_text`'s function-typed-field arm (`storage_field_is_function`) is
  reached BEFORE the general reference-class field arm, so it read
  `recv.field` against the handle newtype — E0609, "available field is: `0`".
  It now projects through `.0.borrow()` like every other declared field.
* calling such a field in place holds the `Ref` guard for the whole statement,
  and the callable read out of it is precisely the arrow that `borrow_mut()`s
  the same cell: "RefCell already borrowed" at run time. Both call emitters
  (`Rvalue::ClosureCall` in `call_runtime.rs` and `call_virtual_function_field_text`
  in `call.rs`) now bind the callable in its own `let` first, which drops the
  guard before the call. `operand_reads_through_ref_cell` is the one predicate
  both consult, and it also covers a callee read out of a shared closure
  capture.

A fourth, smaller consequence: a `Move` operand over a place that lives in a
shared capture cell rendered as `(*smelt_capture_x.borrow())`, which Rust cannot
move out of (E0507 on the constructor's `return`). `operand_text`'s `Move` arm
now clones such a read, with the same `Copy`-scalar and non-cloneable exclusions
the `Copy` arm already had.

## Test

`crates/smelt-codegen-rust/tests/private_member_call_runtime.rs` gains
`a_class_field_arrow_reaches_this_through_a_private_name` (already in the
`functions` shard of the runtime-tier matrix): a private write, a private
compound write (`this.#count++`), a private METHOD call and a private read
(plus an optional-chained one) all from inside class-field arrows, asserted
against the instance the caller holds and against a second, untouched instance.
Expectations are what Node 22 prints.

```sh
cargo test -p smelt-codegen-rust --test private_member_call_runtime -- --ignored
```

## Probe

258 files / 2 with blockers / 2 occurrences -> **258 / 1 / 1**. Both remaining
occurrences went away: `context.ts` moved on to a NEW blocker (`setters are not
lowered yet`, line 414 `set res(_res: Response | undefined)`, which was masked
behind this one), and `hono-base.ts`'s "Response init is an erased value" stopped
being reported at all — it was downstream of `context.ts` failing to lower, so
`Context`'s members were unresolved when `hono-base.ts` was typed.
