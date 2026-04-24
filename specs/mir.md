# MIR (Mid-Level Intermediate Representation)

MIR is smelt's second intermediate representation. It is the target of HIR lowering and the source of Rust codegen. MIR looks like Rust's mental model: control-flow graphs, single-static-assignment, three-address statements, place-based memory.

## Goals

- **Close to Rust.** A MIR function maps onto a Rust function with minimal restructuring.
- **Full SSA with phi nodes.** Every local is assigned exactly once; control-flow merges are explicit. Optimization passes have a real foundation.
- **Place-based reads.** Reads go through `Place`s (local + projection), so v2.0 borrow inference can be added without a structural rewrite. Writes define fresh SSA locals only.
- **Terminator-based control flow.** `if`, `while`, `for`, `try`, and `await` all become CFG edges. No nested control in statement form.
- **Validatable.** A MIR validator catches malformed lowering output (use-before-def, missing terminators, type mismatches, malformed phis).

## Non-Goals (v1.0)

- Borrow inference, lifetimes, ownership analysis. Every value is owned and `Clone`. v2.0 work.
- Inlining, dead-code elimination, constant folding. Codegen emits as-is.
- Memory layout decisions (boxing, repr). Codegen makes those calls.

## Design Principles

1. **Pure SSA.** Every value-producing operation defines a fresh `LocalId`, exactly once. There is no assignment to `Place`, no field/index mutation, and no redefinition of locals. Field or index "updates" produce a fresh aggregate local; codegen may later choose an efficient Rust shape, but MIR itself stays immutable.
2. **Phi nodes at block heads.** Each phi declares a destination local and an incoming `(predecessor_block, operand)` per edge. Simple to validate, round-trips through serde cleanly, well-understood by every textbook optimizer.
3. **Calls are terminators.** Every function call ends a basic block. Async lowering (M5) drops a state-machine state at every call site; keeping calls as terminators makes that mechanical. Synchronous codegen simply emits the call inline because the success edge is unconditional.
4. **No unwind edges.** Exceptions are lowered to `Result` *before* MIR. Calls do not unwind. This is the single decision that makes async lowering tractable.
5. **One MIR function per HIR body.** Methods, lambdas, and comprehensions all become free MIR functions. Closures get a synthesized capture struct as their first parameter.
6. **Types stay interned.** MIR shares the HIR `TypeInterner` (extended with monomorphized instantiations). Codegen does not re-resolve types.

## Top-Level Structure

```rust
pub struct Mir {
    pub functions: IndexVec<FuncId,    MirFunction>,
    pub adts:      IndexVec<AdtId,     Adt>,            // structs/enums (classes, error unions, discriminated unions)
    pub closures:  IndexVec<ClosureId, ClosureLayout>,  // capture structs synthesized for lambdas
    pub types:     TypeInterner,
    pub symbols:   SymbolInterner,
}

pub struct MirFunction {
    pub id:        FuncId,
    pub name:      Symbol,
    pub origin:    HirOrigin,                // back-pointer for diagnostics
    pub is_async:  bool,
    pub params:    Vec<LocalId>,             // first N entries of `locals`
    pub return_ty: TypeId,
    pub locals:    IndexVec<LocalId, LocalDecl>,
    pub blocks:    IndexVec<BlockId, BasicBlock>,
    pub entry:     BlockId,
}

pub struct LocalDecl {
    pub ty:   TypeId,
    pub kind: LocalKind,                     // Param | Temp | UserBinding(Symbol)
    pub span: Span,
}
```

## Basic Blocks

```rust
pub struct BasicBlock {
    pub id:         BlockId,
    pub phis:       Vec<Phi>,
    pub statements: Vec<Statement>,
    pub terminator: Terminator,
}

pub struct Phi {
    pub dest:     LocalId,
    pub ty:       TypeId,
    pub incoming: Vec<(BlockId, Operand)>,
}
```

`phis` execute "in parallel" before the first statement. Validation enforces: each phi has exactly one incoming entry per predecessor; types match; each `Phi::dest` is unique within the block.

## Places, Operands, Rvalues

```rust
pub enum Place {
    Local (LocalId),
    Field { base: Box<Place>, idx: u32,     ty: TypeId },
    Index { base: Box<Place>, idx: LocalId, ty: TypeId },
}

pub enum Operand {
    Copy  (Place),                           // v1.0: lowered to `.clone()` in codegen
    Move  (Place),                           // v1.0: rare; reserved for tail uses
    Const (Constant),
}

pub enum Rvalue {
    Use         (Operand),
    BinOp       (BinOp, Operand, Operand),
    UnaryOp     (UnOp,  Operand),
    Aggregate   (AggregateKind, Vec<Operand>),  // tuple, struct, list/vec, dict
    Cast        (CastKind, Operand, TypeId),
    Closure     { id: ClosureId, captures: Vec<Operand> },
    Discriminant(Place),                        // tag of an enum / discriminated union
}
```

`Move`-vs-`Copy` is meaningless in v1.0 (everything clones). Both variants exist now so the v2.0 ownership pass has tags to refine.

`Place` is read-only in v1.0 MIR. It describes where an operand is read from, including projections. It is never a write destination.

## Statements

```rust
pub enum Statement {
    Assign      { dest: LocalId, value: Rvalue },
    StorageLive (LocalId),                   // optional lifetime marker; not a definition
    StorageDead (LocalId),
}
```

In well-formed v1.0 MIR, every `Statement::Assign { dest, .. }` writes a fresh `LocalId` that has not been defined before. `Place::Field` and `Place::Index` are read projections only. A source assignment such as `x.y = v` lowers to a fresh aggregate value, then a fresh SSA version for `x`; the previous local remains immutable.

## Terminators

```rust
pub enum Terminator {
    Goto    (BlockId),
    SwitchInt {
        discriminant: Operand,
        targets:      Vec<(i128, BlockId)>,
        default:      BlockId,
    },
    Call {
        callee: Callee,
        args:   Vec<Operand>,
        dest:   LocalId,                    // fresh SSA local receiving the call result
        target: BlockId,
    },
    Return  (Operand),
    Unreachable,
}

pub enum Callee {
    Static   (FuncId),
    Indirect (Operand),                      // function-typed local (closure result)
    Builtin  (BuiltinFn),                    // target-neutral stdlib operation (e.g. ListPush, DictGet)
}
```

Only one call per block; the success edge is `target`. There is no `Unwind` edge — exceptions have already been lowered to `Result`.

`BuiltinFn` lives in `smelt-mir`, not codegen. MIR lowering must know the operation's argument and return types for validation. Rust codegen only decides the spelling (`Vec::push`, `HashMap::get`, helper runtime call, etc.).

## ADTs and Synthesized Enums

HIR's classes and discriminated unions become MIR `Adt`s. Exception lowering also synthesizes per-function error enums.

```rust
pub struct Adt {
    pub name:     Symbol,
    pub kind:     AdtKind,                   // Struct | Enum
    pub variants: Vec<Variant>,
    pub generics: Vec<TypeVarId>,            // pre-monomorphization; empty after the mono pass
}

pub struct Variant {
    pub name:   Symbol,
    pub fields: Vec<TypeId>,
}
```

Codegen emits both as Rust `struct`/`enum` declarations with `#[derive(Clone)]`.

## Lowering Passes (HIR → MIR)

Passes run in order. Each lives in its own module under `smelt-mir::lower::*` with unit tests.

1. **HIR validation.** Run `smelt-hir::validate` and stop on errors. Lowering assumes all expressions are typed, locals resolve inside their bodies, matches are exhaustive, and async/exception constraints are already source-valid.
2. **HIR normalization.** Rewrite high-level but language-neutral sugar into a smaller HIR subset before graph construction:
   - `ExprKind::Method` resolves to a known `FunctionId` plus an explicit receiver argument.
   - `ExprKind::If` in expression position becomes branch blocks with a merge value.
   - `Stmt::Assign { target: PlacePath }` becomes a functional update expression that produces a fresh value for the root binding.
3. **ADT layout synthesis.** Emit MIR `Adt`s for classes, discriminated unions, `Optional`, collection helper shapes that need runtime structs, and per-language structural interfaces. This pass fixes field order so later `Place::Field { idx }` is numeric and stable.
4. **Closure capture explicitness.** Walk lambda bodies, identify free locals, build a `ClosureLayout`, and create one free MIR function per lambda body. Captures are cloned into a synthesized capture struct in v1.0.
5. **Exception lowering.** Lift the throw-type set of each function. Synthesize an error enum (`MyFnError`) with one variant per distinct throw type. Rewrite:
   - `Stmt::Throw(e)` → `Return(Err(e))`
   - `try/catch` → `SwitchInt` on the error enum's discriminant after the call.
   - Function return type becomes `Result<T, MyFnError>`.
6. **Generic monomorphization.** For each concrete instantiation observed in the call graph, emit a specialized `MirFunction` and `Adt`. Substitute `TypeVar` → `TypeId` in all owned types. Bounded generics over traits are deferred to v2.0.
7. **Builtin resolution.** Convert language-neutral operations to MIR `BuiltinFn`s: collection construction, list/set insert, dict lookup/insert, string operations, optional/result helpers, and iterator creation. This happens before CFG construction so each builtin call has a known `Callee` and return type.
8. **CFG construction.** Convert HIR's tree-shaped blocks into MIR basic blocks with explicit terminators. `if`, `while`, `for`, `match`, `try`, `await`, `break`, and `continue` become edges. Calls become `Terminator::Call`.
9. **Pure SSA construction.** Define every expression result, call result, phi result, and functional update as a fresh `LocalId`. Insert phi nodes at merge blocks and loop headers. No pass after this may write an existing local.
10. **Async marking.** Set `MirFunction::is_async = true` for functions containing await-derived call sites. Actual state-machine lowering happens in M5; until then, codegen emits `async fn` and lets `rustc` handle it.
11. **Validate.** Run the MIR validator before handing off to codegen.

Pass order is load-bearing: exception lowering runs before monomorphization so synthesized error enums are specialized; builtin resolution runs before CFG construction so calls have stable signatures; pure SSA construction runs after CFG construction so phi placement sees final control flow; no value-producing rewrite runs after pure SSA except validation.

## Validation

`smelt-mir::validate(&Mir) -> Result<(), Vec<ValidationError>>` checks:

- Every `BasicBlock` ends in exactly one `Terminator`.
- Every non-param `LocalId` has exactly one definition, either a `Phi::dest`, `Statement::Assign.dest`, or `Terminator::Call.dest`.
- Every `LocalId` use is dominated by its definition.
- Every `Phi::incoming` has exactly one entry per predecessor block, and types match `Phi::ty`.
- Every `Phi::dest`, `Statement::Assign.dest`, and `Terminator::Call.dest` is unique across the function.
- Every `Place` projects through types that exist in the interner.
- Every `Call::callee` resolves.
- Type of each `Rvalue` matches the type of its destination `LocalId`.
- No `Statement` or `Terminator` writes to `Place::Field` or `Place::Index`; places are read-only projections.
- No `BasicBlock` is unreachable from `entry` (warn, do not error).
- `is_async = true` ⇔ at least one await-derived call site exists (skipped pre-M5).

Validator failures are MIR-pass bugs and crash loudly during development.

## Runtime → MIR mapping (the hard part)

The reason MIR exists is to translate runtime-heavy idioms into things Rust expresses naturally. The mapping is fixed at lowering time, not codegen time, so codegen stays small.

| HIR construct                | MIR shape                                | Codegen emits                       |
|------------------------------|------------------------------------------|-------------------------------------|
| `Stmt::Try`                  | `SwitchInt` on `Result` discriminant     | `match` + `?`                       |
| `Stmt::Throw(e)`             | `Return(Err(e))`                         | `return Err(e)`                     |
| `Expr::Comprehension`        | iterator loop + fresh accumulator locals | `.iter().map().collect()`           |
| `Expr::Lambda`               | free fn + capture struct                 | `move \|args\| body`                |
| `Expr::Method` (single-inh.) | static `Call` w/ self                    | `obj.method(args)`                  |
| `Expr::Await`                | `Call` terminator with `is_async`        | `.await`                            |
| `Stmt::For` over List        | iterator state machine                   | `for x in iter`                     |
| `Stmt::For` over Dict        | `iter()` + tuple destructure             | `for (k, v) in d.iter()`            |
| `Type::Class` with bases     | flattened struct + composition           | `struct`                            |
| `Type::Union` (discriminated)| enum ADT                                 | `enum`                              |
| `Type::Optional<T>`          | `Option<T>` ADT                          | `Option<T>`                         |
| `Type::Future<T>`            | function marked async                    | `async fn -> T`                     |
| `Expr::New { class, args }`  | `Aggregate(Struct, args)`                | `Class { f0: a, f1: b }`            |
| Stdlib `List::push` etc.     | `Call(Callee::Builtin(_))` to fresh dest | `vec.push(x)` / helper call         |

Anything that does not lower into one of the above shapes is a MIR-pass bug, caught by validation.

## v1.0 → v2.0 Trajectory

The design admits the following **without a structural rewrite**:

- **Ownership inference.** `Operand::Copy`/`Move` already exist; a future pass refines them. `Place` already supports projections; borrows are added by introducing `Operand::Borrow(Place, BorrowKind)`.
- **Drop elaboration.** A pass that inserts `Terminator::Drop` at last-use of each owned local is purely additive.
- **Optimization passes.** SSA + CFG is the standard substrate; constant folding, GVN, DCE, and inlining all become tractable.
- **Async state-machine lowering.** Already prepared: every call is a terminator, so each `await` is a single block boundary to split into a state.
- **Multiple backends.** MIR has nothing Rust-specific. Codegen reads it; another codegen could emit Go or LLVM IR.

## Decisions Log

- `Place` projections as `Box<Place>` or as a flat `(PlaceBase, Vec<Projection>)`. Flat is easier to traverse; box matches rustc. Defer to whichever feels better in real code.
- Where dominator information lives — recomputed on demand or cached on `MirFunction`. v1.0: recomputed; cache only if profiling demands.
- **Pure SSA:** write destinations are fresh `LocalId`s only. `Place` is read-only in v1.0 MIR.
- **Builtins:** `BuiltinFn` lives in `smelt-mir`; backend codegen decides only emitted spelling.
