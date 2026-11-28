# Design decisions, November 2026

Status: decided by the project owner on 2026-11-28; C and D revised the same day (C1 and D2 replace C2 and D1, see below). Each section records the problem, the options that were
weighed, the decision, and what the implementation must guarantee. Implementation PRs link back here and
extend the referenced design docs; this file is the record of *what* was chosen and *why*.

## A. Object literals with methods and `this` — decision: A1

**Problem.** An unannotated object literal with methods (`const c = { n: 0, inc() { this.n++; return this.n } }`)
lowers to `SmeltUnknown`, and `this` inside its methods goes through the erased per-call `this`.

**Options.**
- A1: keep the existing record-with-function-fields representation; take the record type the TypeScript checker
  infers for unannotated literals; bind each method to its object through a weak back-reference created during
  construction (`Rc::new_cyclic`), so `this` is a typed receiver.
- A2: generate a struct with real methods and lower interfaces as Rust traits (closest to a hand port, but a
  rewrite of how every interface is emitted).

**Decision: A1.**

**Guarantees.** `this` inside a literal's methods is the literal's own record, typed; no `SmeltUnknown`. Interface
slots can hold such literals directly. Iterator object literals with state or `this` (blocked in
`docs/design/iterator-protocol.md`) are unblocked by this.

**Documented divergence.** A method read off the object (`const f = c.inc; f()`) or copied by spread
(`{ ...c }`) keeps the *original* object as `this`. In JavaScript a detached method gets `this === undefined`
(strict mode) and usually throws; a spread copy's methods see the copy. Only code that detaches methods observes
the difference. Tests must pin this behaviour so a later change is deliberate.

## B. Unions with an optional discriminant — decision: B1

**Problem.** `{ type?: 'one' } | { type: 'two' }` stays erased. Typing the optional field as `Option<T>` makes
`{ type: undefined }` and `{}` indistinguishable, which broke ts-pattern's issue-#278 spec (pin 437/1 → 436/2)
when tried in #566.

**Options.**
- B1: a presence type ("absent / present-undefined / value") only for optional fields whose *presence* the
  program observes — through `in`, `Object.keys`/`entries`/`values`, spread or rest, `hasOwnProperty`/
  `Object.hasOwn`, `JSON.stringify` key enumeration; `Option<T>` everywhere else.
- B2: plain `Option<T>` and accept the divergence.
- B3: leave these unions erased.

**Decision: B1.** This is a scoped exception to the "no tri-state ABI" rule (`Option<T>` stays the default for
`T | null | undefined`).

**Guarantees.** The presence type appears only where a presence observation exists in the program (a
whole-program fact computed before lowering); a field never observed for presence stays `Option<T>`. Narrowing by
the optional discriminant is typed (extends #553's literal narrowing). The ts-pattern pin must hold.

## C. Function identity — decision: C1 (revised; replaces C2)

**Problem.** A function used where an object type is expected is wrapped into a record view, so identity is
lost: after `const n: Named = hello`, `n === hello` is `false` (Node: `true`).

**Options.**
- C1: every function value that can be wrapped carries a small identity token; the record built when it is
  wrapped copies the token; `===` and `Object.is` compare tokens.
- C2: represent a function that has (or acquires) properties as a single callable object from creation.
- C3: keep `false` and document it as a limit of the subset.

**Decision: C1.** C2 was chosen first, then replaced after the product direction settled on idiomatic Rust
output and Rc-leak safety: C2 turns every such function into an `Rc<RefCell<…>>` callable with dynamic dispatch
(non-idiomatic) and makes self-referencing closures strong `Rc` cycles (a leak class).

**Guarantees.** No new `Rc`, `RefCell` or back-reference; plain functions never wrapped stay plain `fn`s/closures;
the only cost is one id field on the wrapping record. `===`/`Object.is` between a function and any record that
wraps it agree with Node. Property writes through a wrapping record are not seen through the bare function (and
vice versa): that remains a documented limit, not covered by C1.

## D. `import * as NS` used as a value — decision: D2 (revised; replaces D1)

**Problem.** Using a namespace import as a value (`Object.keys(NS)`, passing `NS` to a function, `NS[k]` with a
literal-union `k`) erases it ("imported value" / untyped-package).

**Options.**
- D1: a generated struct per module with live accessors (thread-local `RefCell` state behind accessor functions).
- D2: a plain typed struct of the module's exports (constants and function pointers), built where needed.
- D3: keep erased.

**Decision: D2.** D1 was chosen first, then replaced for idiomatic output: live bindings need a global mutable
layer hand-written Rust would not have. D2 is exact for `export const` and functions, which never change.

**Guarantees.** `NS.x`, `NS` passed as a value, `Object.keys(NS)` in spec order (sorted export names) and
`NS[k]` for a literal-union `k` (a typed match) are typed, no `SmeltUnknown`. A module that exports a mutable
`export let` read through a namespace value is refused with a clear diagnostic naming the binding. Writes through
`NS` are a type error in TypeScript already. No `Rc` cycles, no thread-local state.
