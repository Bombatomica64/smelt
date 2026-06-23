# MIR optimization roadmap

Smelt lowers source (TypeScript / Python) → HIR → MIR → Rust. The MIR layer
exists so we can apply the optimizations a human performs **when hand-porting a
GC'd, reference-semantics language to Rust** — decisions the Rust compiler will
*not* make for us, because they happen before we commit to a clone, an owned
ABI, or an `Rc<RefCell<_>>`.

This document records the guiding principle, what the MIR actually is, the
machinery we can reuse, and the ordered list of passes.

## Guiding principle: don't redo rustc/LLVM

We only spend effort on transformations that the Rust compiler structurally
cannot recover:

| Transformation | rustc/LLVM already does it? | Worth a MIR pass? |
| --- | --- | --- |
| Constant folding, dead-local elimination, register allocation, inlining, loop unrolling | yes | **no — skip** |
| `.clone()` on `Copy` types (`f64`, `bool`, `i64`) | elided at runtime | cosmetic only (readability + clippy) |
| `.clone()` on `String` / `Vec` / structs / `Rc` that could be a move | **never** — a written clone always allocates | **yes — biggest win** |
| Passing `&T` instead of cloning an owned arg the callee only reads | no — it is our ABI choice | **yes** |
| `Rc<RefCell<T>>` → owned `T` when single-owner | no | **yes (harder)** |
| Iterator fusion (drop the intermediate `Vec` in map/filter/reduce) | no — we emitted the `.collect()` | yes (medium) |
| `String` vs `&str`, int vs `f64` specialization | no | yes (needs frontend/type work) |

The unifying theme is **ownership inference**: figure out who owns each value
and who last touches it, then *move* or *borrow* instead of *clone*.

## What the MIR is

A CFG of basic blocks (`MirFunction { blocks, entry, locals, params }`), each
block holding `phis` + `statements` + a `terminator`.

It is a **hybrid, not pure SSA**: it has SSA-style `Phi` nodes *but also*
mutable locals — `Statement::AssignPlace` writes through `Place::Field` /
`Place::Index`, a local can be assigned more than once, and `mutated_locals`
already tracks in-place mutation. No pass may assume single assignment.

Every value read is an `Operand`:

```rust
enum Operand { Copy(Place), Move(Place), Const(Constant) }
enum Place   { Local(LocalId), Field { base, field }, Index { base, index } }
```

The entire `.clone()` surface funnels through one decision in codegen
(`emitter/core.rs::operand_text`): **`Operand::Copy` → `.clone()`,
`Operand::Move` → no clone** (function-typed and non-cloneable places already
skip the clone). So the first wins need *no codegen change* — only flipping
`Copy` → `Move` (and, later, ABI rewrites) in MIR.

## Machinery to reuse

- **CFG successors** — `validate::successors(&Terminator)` handles every
  terminator (Goto / Call+unwind / Await+unwind / Switch / Match+default /
  Return / Throw / Unreachable). Promote to `pub(crate)`.
- **Read-side operand visitor** — `Rvalue::for_each_operand(|&Operand|)` in
  `validate.rs` is the documented single source of truth; it visits every
  operand of all ~150 `Rvalue` variants in evaluation order. Use it for GEN
  sets. Add a symmetric `for_each_operand_mut` (compiler-checked exhaustive)
  for the rewrite step; later passes reuse it too.
- **Escape analysis** — `lower::closures::mark_escaping_closures` plus
  `MirClosureCapture` track which closures/locals escape; reusable to forbid
  moving captured-by-ref locals (Pass 3 especially).
- **Pass infrastructure** — the `Pass` trait + fixpoint `optimize()` driver in
  `opt/`. New passes register in `default_passes()`.
- Helpers already present: `mutated_locals`, `assigned_local_counts`,
  `is_temp_local`, `is_function_local`, type access via `mir.types`.

## What is missing

1. **Liveness dataflow** — none today. The existing `CopyPropagation` uses only
   whole-function *counts*, never CFG-aware live-in/live-out. Move-on-last-use
   requires real backward liveness: a use is "last" only if its local is not
   live-out along *any* successor (loop back-edges make a textually-last use
   non-final).
2. **Mutable operand visitor** (`for_each_operand_mut`) for the rewrite.
3. `Statement::StorageDead` is emitted but ignored by codegen, and is not a
   reliable liveness oracle — compute liveness rather than trust it.

## The passes, in order

### Pass 1 — Move-on-last-use (the foundation) — *done*

Implemented in `crates/smelt-mir/src/opt/move_on_last_use.rs`. Backward liveness
over the CFG; rewrite `Operand::Copy(Place::Local(x))` → `Operand::Move` at any
use where `x` is dead immediately after. Kills real `String`/`Vec`/struct
allocations rustc cannot (e.g. `counter = _smelt_tmp_1;` moving an owned struct,
`last = name;` moving a dict key, accumulator `x = x + v` moving `x`), and clears
`Copy`-type clone noise as a side effect.

**Correctness guards (must all hold to convert):**

- Only `Place::Local`. Moving out of `Place::Field` / `Place::Index` is illegal
  Rust (`cannot move out of ...`); those operands keep `Copy`.
- The local is dead in the live-out set at that program point, and read exactly
  once in the containing statement/terminator (avoids `x + x` ordering hazards).
- Exclusions: parameters (their ABI belongs to Pass 2; receivers may be
  references), any local captured by a closure (a by-ref capture outlives the
  textual use), and any local touched by a `Phi` (edge-precise phi liveness
  deferred — these locals are excluded so ignoring phi edges stays sound).
- A move *before* a later reassignment of the same local is fine — liveness
  already accounts for it (the local is dead between move and re-def). This is
  what lets accumulator patterns move.
- **`Switch`/`Match` scrutinees are left as `Copy`.** The Rust emitter rebuilds
  structured loops by pattern-matching a header of the exact shape
  `switch copy %local` (`emitter/control_flow.rs`); a `move` scrutinee silently
  fails that match and flattens nested loops. Scrutinees are always `bool`/tag
  (`Copy`) values, so this costs nothing at runtime. A future cleanup could make
  the emitter accept `Copy | Move` and drop this exclusion.

**Dataflow specifics:**

- KILL: `Assign { dest }`, `AssignPlace { place: Local(l) }` define their
  target; `Call`/`Await` define `dest`.
- GEN (reads): all operands surfaced by `for_each_operand`, decomposed —
  `Copy/Move(Local l)` reads `l`; `Field { base }` reads `base`;
  `Index { base, index }` reads `base` and the `index` operand. `AssignPlace`
  through a `Field`/`Index` place is a **read** of `base` (mutated in place,
  stays live), not a kill.
- Phis: dest is defined at block entry; incoming operands are uses on the
  predecessor edge. v1 sidesteps edge precision via the phi exclusion above.

**Verification:** golden `expected.rs` diffs (e.g. `22_mutating_method` should
lose its movable clones), plus MIR `validate` still passing.

### Pass 2 — Borrow-instead-of-clone for arguments

When a callee only *reads* a parameter, change its ABI to `&T` and drop the
clone at call sites. Needs Pass 1's liveness plus a per-function
"param is read-only" summary. Touches signatures → emits changes, so it lands
after Pass 1 is stable.

### Pass 3 — `Rc<RefCell<T>>` → owned `T`

Use the existing escape analysis: single-owner, non-escaping shared cells become
plain owned values, removing refcount traffic, runtime borrow checks, and heap
indirection. Highest structural payoff, most invasive — wants Passes 1–2 stable.

### Later

- **Iterator fusion** — drop intermediate `Vec`s in map/filter/reduce chains
  (we emit the `.collect()`, so rustc will not fuse them).
- **Specialization** — int vs `f64`, `&str` vs `String`; needs frontend / type
  cooperation, so sequenced after the ownership passes.
