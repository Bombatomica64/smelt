# Design decisions, November 2026

Status: decided by the project owner on 2026-11-28. Each section records the problem, the options that were
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

## C. Function identity — decision: C2

**Problem.** A function used where an object type is expected is wrapped into a record view, so identity is
lost: after `const n: Named = hello`, `n === hello` is `false` (Node: `true`), and properties written through one
are not seen through the other.

**Options.**
- C1: every function value carries a stable identity, record views carry it, `===`/`Object.is` compare it.
- C2: represent a function that has (or acquires) properties as a single callable object from creation: one
  shared allocation holding the callable and its property slots, so every view is the same object.

**Decision: C2** (chosen over the C1 recommendation): identity correct by construction rather than by a rule every
wrapping site must uphold.

**Guarantees.** For a function value that is read as an object (property reads/writes, `name`/`length`, use at an
object or callable-interface type), the function and every view of it are one reference: `===`, `Object.is`, and
property writes agree with Node. Plain functions never used as objects keep their current lean representation
(no cost for code that does not observe the object side). Extends `docs/design/record-identity.md`.

## D. `import * as NS` used as a value — decision: D1

**Problem.** Using a namespace import as a value (`Object.keys(NS)`, passing `NS` to a function, `NS[k]` with a
literal-union `k`) erases it ("imported value" / untyped-package).

**Options.**
- D1: a generated, typed struct per module whose accessors read the module's live bindings; writes through the
  namespace throw a `TypeError` as in JavaScript (module namespace objects are frozen).
- D2: a typed snapshot record at import time (misses live-binding updates).
- D3: keep erased.

**Decision: D1.** This is step 4 of `docs/design/record-identity.md`.

**Guarantees.** Reads through the namespace see later updates to `export let` bindings; key enumeration order
matches the spec (sorted export names); `NS[k]` with a literal-union `k` is a typed match over exports; writes
and deletes throw; identity is stable (`NS === NS2` for two imports of the same module).
