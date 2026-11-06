# MIR (Mid-Level Intermediate Representation)

MIR is smelt's second intermediate representation: the target of HIR lowering
(`smelt_mir::lower_hir`) and the source of Rust codegen (`emit_source`). A body is
a control-flow graph of basic blocks over typed locals, close enough to Rust that
a MIR function maps onto a Rust function with little restructuring.

The types live in `crates/smelt-mir/src/types.rs`; this document states the shape
and the rules, and the code is authoritative for every field.

## Design Principles

1. **A CFG over mutable locals.** MIR is not SSA and has no phi nodes. A control
   merge is a local assigned on each incoming path, and `MirFunction::assignment_count`
   tells a pass whether a local has one definition.
2. **Place-based reads and writes.** A `Place` names a local or a projection of
   one. `Statement::AssignPlace` writes through a projection.
3. **Calls are terminators when they can leave.** `Terminator::Call` and
   `Terminator::Await` carry an optional `unwind` edge, and only they do, so
   anything that can throw has to be one of them. A throw with no handler in scope
   leaves through the function's error channel: the throwing pass sets
   `MirFunction::can_throw`, and codegen emits a `Result`.
4. **Ops are stated once.** `ExprKind` and `Rvalue` are generated from one table,
   `smelt-hir/src/op_table.rs`, with each op's walkers, dump text and per-op facts.
5. **Types stay interned.** MIR shares the HIR `TypeInterner`. Lowering owns the
   table; codegen, which holds `&Mir`, can name a type the program does not spell
   through `TypeInterner::intern_late`, but such a type is invisible to `all()` and
   `lookup`, so nothing is declared for it.
6. **Language stays with the body.** `MirFunction::language` records which frontend
   a body came from, for the rules the operand and its type do not determine.

## Top-Level Structure

```rust
pub struct Mir {
    pub functions: Vec<MirFunction>,
    pub classes:   Vec<MirClass>,
    pub interfaces: Vec<MirInterface>,
    pub closures:  Vec<MirClosure>,
    pub globals:   Vec<MirGlobal>,        // module-level mutable bindings
    pub types:     TypeInterner,
    pub symbols:   SymbolInterner,
    // ..
}
```

A `MirFunction` owns its `locals` and `blocks`, an `entry` block, its `params` and
`return_ty`, and the flags `is_async`, `is_generator` and `can_throw`. A closure body
is a `MirClosure` with its captures; it is emitted through a synthesized
`MirFunction`.

## Blocks, Statements, Terminators

A `BasicBlock` is a list of statements and one terminator (`None` only while a body
is being built).

```rust
pub enum Statement {
    Assign { dest: LocalId, value: Rvalue },
    AssignPlace { place: Place, value: Rvalue },
    DictEntryUpdate { .. },               // read-modify-write of one dict entry, in place
}

pub enum Terminator {
    Goto(BlockId),
    Call { callee: Callee, args: Vec<Operand>, dest: LocalId, target: BlockId,
           unwind: Option<ExceptionHandler> },
    Await { future: Operand, dest: LocalId, target: BlockId,
            unwind: Option<ExceptionHandler>, .. },
    Switch { cond: Operand, .. },
    Match { scrutinee: Operand, .. },
    Return(Operand),
    Throw(Operand),
}

pub enum Callee {
    Static(FuncId),
    Indirect(Operand),
    Builtin(BuiltinFn),                   // the console family
    Op(Box<Rvalue>),                      // an operation that can throw
    BaseInit(FuncId),                     // a derived constructor's `super(..)`
}
```

`Callee::Op` is how an operation reaches an unwind edge. Each op states
`fallible(..)` in the table; lowering asks `Rvalue::is_fallible` and places the op
in an assignment or in a call. The op keeps its own typed fields, so `args` is empty
for it: read a terminator's operands through `Terminator::reads`, never through
`args` alone.

## Places, Operands, Rvalues

```rust
pub enum Place {
    Local(LocalId),
    Field { base: LocalId, field: Symbol },
    Index { base: LocalId, index: Box<Operand>, negative: NegativeIndex },
    Global { .. },                        // a projection rooted at a mutable global
}

pub enum Operand { Copy(Place), Move(Place), Const(Constant) }
```

`Move` marks a last use (`opt::move_on_last_use`), which lets codegen skip a clone.

`Rvalue` is the op table's entries plus the forms only MIR has: `Use`, `Binary`,
`Unary`, the aggregates `List`, `Set`, `Dict`, `Tuple`, `Struct`, `Closure`,
`DictEntryPush` (a push through a dict entry, in place), and the receiver-resolved
reads `BoundField`, `BoundMethod`, `UnionMethod`.

## Traversal

`traversal.rs` owns what a statement, terminator and rvalue read and define:
`reads`, `reads_mut`, `defs`, `read_locals`. Optimization passes, the validator and
the emitter all ask through it, so a new form is described once. Operands are
visited in evaluation order.

## Lowering

`lower_hir` lowers every body, then `run_finalization_passes` runs a fixed order,
each step stating what it requires and establishes:

1. `generic_records`: every generic record instantiation is in the type table.
2. Closure escape marking.
3. The throwing fixpoint: `can_throw`, and the throwing ABI on function-typed locals.
4. `promote_erased_mutated_records`.
5. `normalize_operational_types`.
6. `inherited_types`: every type an inherited member copy is emitted at is in the
   table.

Steps 1 and 6 are required because codegen declares unions and record helpers from
the table before it renders any body. `smelt_mir::compile` runs lowering, `opt` and
validation.

## Validation

`smelt_mir::validate` reports a flat list of `ValidationError`s:

- `structure`: blocks, locals and callees referenced exist.
- `operands`: each statement and rvalue reads valid places.
- `assignment`: forward definite assignment, catching a read before a definition.
- `closures`: the closure table and capture ABI.

Validator failures are MIR-pass bugs and crash loudly during development.
