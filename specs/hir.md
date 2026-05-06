# HIR (High-Level Intermediate Representation)

The HIR is smelt's first intermediate representation. Both the TypeScript and Python frontends produce HIR. All language-specific concerns end here; everything downstream operates on HIR or its lowered form (MIR).

## Goals

- **Language-agnostic.** Nothing in HIR should look TS-specific or Python-specific.
- **Fully typed.** Every expression carries a resolved type. There is no inference at the HIR level — that is the frontend's job.
- **High-level enough to be readable.** Classes, methods, exceptions, comprehensions, and async/await are still recognizable. Lowering them is MIR's job.
- **Stable across passes.** Lowering, validation, and analysis attach side tables keyed by HIR IDs; they never mutate HIR in place.
- **Serde-serializable.** Snapshot tests dump HIR for diffing.

## Non-Goals

- Optimization. HIR is for *correctness* of language translation, not performance.
- Source-form fidelity. HIR doesn't need to round-trip back to TS/Python.

## Design Principles

These shape the data layout. They follow how mature compilers (rust-analyzer, rustc) structure their IRs so that future-us can add incremental compilation, query-based analysis, and arena tuning without rewriting call sites.

1. **IDs everywhere, references nowhere.** No `Box<Expr>`, no `&Expr`. Every node lives in an arena (`IndexVec`) on its owning `Body` or `Crate`. Cross-references are typed indices (`ExprId`, `ItemId`, `TypeId`). Avoids lifetime gymnastics, keeps cycles representable, and makes serialization straightforward.
2. **Bodies are separate from items.** Item signatures (`Function`, `Class`) live on the `Crate`; their executable bodies (`Body`) live in a parallel arena. A change to a function body does not invalidate analyses keyed on signatures. This is the rust-analyzer split, and it is what makes incremental compilation possible later.
3. **Types are interned.** `TypeId(u32)` → a deduplicated `Type` in a `TypeInterner`. Type equality becomes an integer compare.
4. **Symbols are interned globally per crate.** `Symbol(u32)` → an interned string. Cross-language imports (M9) need consistent identity across modules; per-module interning would force re-interning at every boundary.
5. **Side tables, not mutation.** Capture analysis, monomorphization, and codegen attach `HashMap<HirId, T>` (or `IndexVec`) side tables. HIR is constructed once and frozen.
6. **Spans are first-class.** Every item, expression, statement, block, and pattern carries a `Span = (FileId, ByteRange)`. Errors are usable.

## Top-Level Structure

```rust
pub struct Crate {
    pub modules: IndexVec<ModuleId, Module>,
    pub items:   IndexVec<ItemId,   Item>,
    pub bodies:  IndexVec<BodyId,   Body>,
    pub types:   TypeInterner,
    pub symbols: SymbolInterner,
    pub names:   OriginalNameTable,        // canonical → source-spelled name
}

pub struct Module {
    pub id:      ModuleId,
    pub name:    ModuleName,               // canonical snake_case
    pub source:  SourceFile,               // path + Language tag
    pub imports: Vec<Import>,
    pub items:   Vec<ItemId>,
}

pub enum Item {
    Function(Function),
    Class(Class),
    TypeAlias(TypeAlias),
    Const(ConstItem),
}
```

**`Crate`** is the root of the whole compilation unit. It owns every arena — all modules, all items, all bodies, all interned types and symbols. Nothing else allocates; everything else is an index into one of these tables.

**`Module`** is one source file. `source` records the original path and whether it was TypeScript or Python. `items` is a list of IDs pointing into the crate-level item table — not the items themselves, just indices.

**`Item`** is anything that lives at module scope: a function, a class, a type alias, or a top-level constant. Items carry only their *signature* (name, types, annotations). Their bodies — the actual executable code — are stored separately.

Items hold *signatures* and metadata. Their executable parts live in `Body`:

```rust
pub struct Body {
    pub id:       BodyId,
    pub owner:    ItemId,                          // function or method this body belongs to
    pub locals:   IndexVec<LocalId,   LocalDecl>,  // params first, then declared bindings
    pub params:   Vec<LocalId>,
    pub exprs:    IndexVec<ExprId,    Expr>,
    pub stmts:    IndexVec<StmtId,    Stmt>,
    pub blocks:   IndexVec<BlockId,   Block>,
    pub patterns: IndexVec<PatternId, Pattern>,
    pub root:     BlockId,
}
```

**`Body`** holds everything that executes at runtime for one function or method. All its sub-nodes (locals, expressions, statements, blocks, patterns) live in flat `IndexVec` arenas indexed by typed IDs. `root` is the entry `BlockId` — the top-level block of the function body. `params` lists the `LocalId`s that correspond to function parameters, always at the front of `locals`.

A `HirId` (used by side tables) is the pair `(BodyId, LocalIndex)` where `LocalIndex` is one of `ExprId`, `StmtId`, `BlockId`, `PatternId`.

## Identifiers

```rust
pub struct ModuleId(u32);
pub struct ItemId(u32);                 // crate-global item index
pub struct BodyId(u32);
pub struct ClassId(ItemId);             // typed view of an item known to be a class
pub struct FunctionId(ItemId);
pub struct LocalId(u32);                // scoped to a body
pub struct ExprId(u32);                 // scoped to a body
pub struct StmtId(u32);
pub struct BlockId(u32);
pub struct PatternId(u32);
pub struct TypeVarId(u32);
pub struct Symbol(u32);                 // interned string, global per crate
pub struct TypeId(u32);                 // interned type
```

All IDs are newtype wrappers around `u32`. The point is that `ExprId(3)` and `LocalId(3)` are different types and can't be mixed up at compile time. `ItemId` is crate-global and indexes `Crate::items`; a module records ownership by listing the `ItemId`s it declares. `ClassId` and `FunctionId` are "typed views" of an `ItemId`: they carry the same data but tell you statically that the item is definitely a class or a function, avoiding runtime enum matching when you already know the kind.

## Types

```rust
pub enum Type {
    // Primitives
    Bool, Int, Float, String, None,

    // Composites
    List(TypeId),
    Set(TypeId),
    Dict(TypeId, TypeId),
    Tuple(Vec<TypeId>),
    Optional(TypeId),
    Union(Vec<TypeId>),                  // discriminated unions only

    // User-defined
    Class { id: ClassId, args: Vec<TypeId> },
    TypeVar(TypeVarId),
    Function(FunctionType),

    // Async
    Future(TypeId),
}

pub struct FunctionType {
    pub params:    Vec<TypeId>,
    pub return_ty: TypeId,
    pub is_async:  bool,
}
```

**Primitives** map directly to Rust scalars. `None` covers `null`, `undefined`, and `void` — they all mean "no value" in HIR.

**Composites:** `List` is a typed homogeneous sequence. `Set` is a typed homogeneous uniqueness collection. `Dict` is a key→value map (both keys and values are typed). `Tuple` is a fixed-length heterogeneous product. `Optional(T)` is `T | null` — it lowers to `Option<T>` in Rust. `Union` is *only* for discriminated unions (a tagged sum type where a literal field tells you which variant you're in); untagged unions are rejected by the frontends.

**User-defined:** `Class` references a `ClassId` plus any type arguments for generic instantiation. `TypeVar` is an unresolved generic parameter (e.g. `T` in `fn foo<T>(x: T)`). `Function` is a first-class function type (used when a function is passed as a value).

**Async:** `Future(T)` represents `Promise<T>` (TS) or `Awaitable[T]` (Python). A function with `is_async: true` returns `Future(its_return_type)`.

`FunctionType` is the type *of* a callable value — its parameter types, return type, and whether it's async. Used when storing a function reference or passing one as an argument.

There is no `Any`. Frontends reject anything that would need it.

## Expressions

```rust
pub struct Expr {
    pub kind: ExprKind,
    pub ty:   TypeId,
    pub span: Span,
}

pub enum ExprKind {
    Literal(Literal),
    Local(LocalId),
    Item(ItemId),                                                // free-function reference
    Call          { callee:  ExprId,   args:   Vec<ExprId> },
    Method        { receiver: ExprId,  method: Symbol,  args: Vec<ExprId> },
    Field         { receiver: ExprId,  field:  Symbol },
    Index         { receiver: ExprId,  index:  ExprId },
    BinOp         { op: BinOp,         lhs: ExprId, rhs: ExprId },
    UnaryOp       { op: UnaryOp,       operand: ExprId },
    If            { cond: ExprId,      then_block: BlockId, else_block: Option<BlockId> },
    Match         { scrutinee: ExprId, arms: Vec<MatchArm> },
    Lambda        { body: BodyId, return_ty: TypeId },
    Await         (ExprId),
    Block         (BlockId),                                     // expression-position block
    ListLit       (Vec<ExprId>),
    SetLit        (Vec<ExprId>),
    DictLit       (Vec<(ExprId, ExprId)>),
    TupleLit      (Vec<ExprId>),
    Comprehension (Comprehension),                               // see below
    New           { class: ClassId, args: Vec<ExprId> },
}
```

**`Expr`** is a wrapper that pairs an `ExprKind` with its resolved type and source span. Every expression in a body is stored as an `Expr` in `Body::exprs`; references to it from other nodes use its `ExprId`.

**`ExprKind` variants:**
- `Literal` — a compile-time constant (number, string, bool, null).
- `Local` — reads the value of a local variable declared in the same body.
- `Item` — references a module-level function by its `ItemId` (a free function used as a value, e.g. passing `my_fn` as a callback).
- `Call` — calls any expression that resolves to a `Function` type; callee is itself an `ExprId`.
- `Method` — calls a named method on a receiver whose class is statically known; lowered to a plain `Call` with `self` in MIR.
- `Field` — reads a named field from a class instance.
- `Index` — subscript access, e.g. `list[i]` or `dict[key]`.
- `BinOp` / `UnaryOp` — arithmetic, comparison, logical operators.
- `If` — an if expression (both if-statements and ternary-style uses share this shape).
- `Match` — pattern-match on a discriminated union; the only way to destructure a `Union` type.
- `Lambda` — an anonymous function with its own `BodyId`; its parameters are `Body::params` in that body. Captures are identified later by the closure-capture pass. This keeps `LocalId` strictly body-scoped.
- `Await` — awaits a `Future`; only valid inside an async body.
- `Block` — an expression-position block, e.g. Rust's `{ stmt; stmt; expr }`.
- `ListLit` / `SetLit` / `DictLit` / `TupleLit` — collection literals.
- `Comprehension` — list/dict/set comprehension or generator; kept high-level, lowered in MIR.
- `New` — construct a class instance, e.g. `Point(x, y)` in Python or `new Point(x, y)` in TS.

Comprehensions stay as a high-level node so the rewrite to iterator chains lives in MIR, not in the frontends:

```rust
pub struct Comprehension {
    pub kind:  ComprehensionKind,           // List | Dict | Set
    pub var:   PatternId,
    pub iter:  ExprId,
    pub guard: Option<ExprId>,
    pub body:  ExprId,
}
```

**`Comprehension`:** `var` is the loop variable (a pattern, e.g. `(k, v)` for dict items). `iter` is the iterable expression. `guard` is the optional `if` filter (`[x for x in xs if x > 0]`). `body` is the expression evaluated per iteration — the value that goes into the result collection. `kind` says whether the result is a list, dict, or set. Lazy generators are rejected in v1.0 because HIR has no generator type.

## Statements and Blocks

```rust
pub struct Block {
    pub stmts: Vec<StmtId>,
    pub tail:  Option<ExprId>,              // expression-block result; None for statement blocks
    pub span:  Span,
}

pub enum Stmt {
    Let      { pat: PatternId, ty: TypeId, value: Option<ExprId> },
    Assign   { target: PlacePath, value: ExprId },
    Expr     (ExprId),
    Return   (Option<ExprId>),
    If       { cond: ExprId, then_block: BlockId, else_block: Option<BlockId> },
    While    { cond: ExprId, body: BlockId },
    For      { pat: PatternId, iter: ExprId, body: BlockId },
    Try      { body: BlockId, handlers: Vec<ExceptionHandler>, finally: Option<BlockId> },
    Throw    (ExprId),
    Break,
    Continue,
}

pub struct PlacePath {
    pub root:     LocalId,
    pub segments: Vec<PathSegment>,         // Field(Symbol) | Index(ExprId)
}
```

**`Block`** is an ordered list of statements plus an optional tail expression. The tail is the block's value when used in expression position (like Rust's `{ …; value }`); it's `None` for plain statement blocks like function bodies. Every block of code in HIR — function bodies, if branches, loop bodies — is a `Block`.

**`Stmt` variants:**
- `Let` — declares a new local variable, binding a pattern to an optional initial value. The type is always known (frontends reject untyped lets).
- `Assign` — writes to an existing place (a local, or a field/index of one). Uses `PlacePath` to describe the target.
- `Expr` — evaluates an expression for its side effects (e.g. a function call whose return value is discarded).
- `Return` — exits the current function, optionally with a value.
- `If` / `While` / `For` — control flow. `For` iterates over any typed iterable; its variable binding is a full `Pattern` so tuple destructuring works.
- `Try` — a try/catch/finally block; exceptions are still present here. MIR lowering first routes throws to lexical catches where possible, then leaves only uncaught throws for Rust `Result` codegen.
- `Throw` — raises an exception. If a surrounding catch handles it, MIR lowers it to a catch edge; otherwise it becomes an uncaught throwing terminator and forces `Result` codegen for the function and its uncaught callers.
- `Break` / `Continue` — loop control.

**`PlacePath`** describes a write target as a root local plus a chain of field accesses and index operations, e.g. `self.points[i]` would be `root=self, segments=[Field("points"), Index(i)]`. This is the only place in HIR where you descend into a value to write part of it.

Exceptions remain in HIR; lowering to `Result<T, E>` happens in HIR → MIR.

## Patterns

Patterns appear in `Let`, `For`, and `Match`:

```rust
pub enum Pattern {
    Wildcard,
    Binding (LocalId),
    Tuple   (Vec<PatternId>),
    Variant { class: ClassId, fields: Vec<(Symbol, PatternId)> },
    Literal (Literal),
}
```

**`Pattern` variants:**
- `Wildcard` — `_`, ignores the value.
- `Binding` — binds the matched value to a `LocalId`; introduces that local into scope.
- `Tuple` — destructures a tuple, recursing into sub-patterns per element.
- `Variant` — matches one arm of a discriminated union, binding named fields; this is how you unpack a `Union` type.
- `Literal` — matches a specific constant value (used in `Match` arms on tagged unions where the tag is a literal).

## Functions and Classes

```rust
pub struct Function {
    pub name:      Symbol,
    pub span:      Span,
    pub params:    Vec<Param>,
    pub return_ty: TypeId,
    pub is_async:  bool,
    pub generics:  Vec<TypeVarId>,
    pub body:      BodyId,
}

pub struct Class {
    pub name:     Symbol,
    pub span:     Span,
    pub fields:   Vec<Field>,
    pub methods:  Vec<FunctionId>,          // each method is its own item with its own Body
    pub generics: Vec<TypeVarId>,
    pub bases:    Vec<ClassRef>,            // single inheritance in v1.0
}
```

**`Function`** is an item's *signature only* — name, parameter list, return type, and a `BodyId` pointing at the executable code. `generics` lists unresolved type variables; they get monomorphized in MIR. `is_async` is set whenever the source function is `async def` / `async function`.

**`Class`** is a struct-like item. `fields` are the data members (name + type). `methods` are `FunctionId`s pointing at `Function` items stored elsewhere in the crate — not nested inside the class. `bases` records the parent class for single inheritance; v1.0 rejects more than one. `generics` are unresolved type parameters shared by fields and methods.

## Naming and Original Names

The HIR canonical form is `snake_case`. The TypeScript frontend converts `camelCase` at the boundary. The `OriginalNameTable` maps every `Symbol` back to its source spelling so diagnostics show the user's own names.

## Validation

`smelt-hir::validate(&Crate) -> Result<(), Vec<ValidationError>>` checks:

- Every `ExprId` resolves to an `Expr` with a defined `TypeId`.
- Every `LocalId` referenced by an `Expr` is declared in the same `Body`.
- Every `ClassId`, `FunctionId`, `TypeVarId`, and `Symbol` resolves.
- `Await` only appears inside a `Body` whose owner has `is_async: true`.
- No `Type::Union` is empty; all members of a union have a literal discriminator field.
- `Match` arms are exhaustive over discriminated unions.
- Every `Body` reaches `Return` or falls off its tail expression.

Validation failures are *frontend bugs*, not user errors. They crash loudly during development and run automatically on every snapshot test.

## Runtime → HIR Mapping (the contract the frontends must enforce)

Mapping runtime-heavy languages onto plain Rust is the project's central problem. HIR's job is to **strip dynamism at the boundary**, before any lowering happens. The frontends accept only what fits the table below; everything else is rejected with a source-located error.

| Source idiom                       | HIR representation                              | Rejected if…                              |
|------------------------------------|-------------------------------------------------|-------------------------------------------|
| `let x = expr`                     | `Stmt::Let { ty: inferred }`                    | type cannot be inferred                   |
| `class C extends B`                | `Class { bases: [B] }`                          | multiple inheritance                      |
| `try { … } catch (e) { … }`        | `Stmt::Try`                                     | exception type is `any`                   |
| `await p`                          | `Expr::Await`                                   | enclosing function is not async           |
| `[x for x in xs]`                  | `Expr::Comprehension`                           | iterates over an `Any`                    |
| `obj.method()`                     | `Expr::Method`                                  | `obj` is not a known class                |
| `dict[key]`                        | `Expr::Index`                                   | dict's value type is `Any`                |
| `==` (TS)                          | `BinOp::Eq` only on same-type operands          | mixed-type comparison                     |
| `for k, v in d.items()` (Py)       | `Stmt::For` over a tuple pattern                | iter target is untyped                    |
| `with f as h:` (Py)                | desugared to `Stmt::Try { finally }`            | context manager protocol unknown          |
| `@decorator` (Py/TS)               | applied in the frontend; never enters HIR       | decorator changes types/control-flow      |

Anything outside this table is a frontend `unsupported` error before HIR is produced. This is the contract MIR depends on.

## Decisions Log (resolves M2 open questions)

- **Symbol interning:** global per `Crate`. Cross-language imports need consistent identity.
- **Decorators:** lowered in the frontend, never enter HIR. Decorators that change semantics outside the v1.0 subset are rejected.
- **Python `with`:** desugared to `Stmt::Try` with `finally` in the Python frontend before HIR.
- **TypeVars:** kept symbolic. Monomorphization is an HIR → MIR pass.
- **Bodies vs inline expressions:** every executable region is in a `Body` arena. Items reference bodies by `BodyId`.
- **Item identity:** `ItemId` is crate-global. Modules own items by listing their IDs; cross-module references do not carry module-local indices.
- **Lambda bodies:** lambdas own a separate `BodyId`. `LocalId` never crosses a body boundary.
- **Comprehension collections:** list, dict, and set comprehensions are supported. Lazy generators are rejected in v1.0.

## What HIR Does Not Have

- Lifetimes, borrows, places-with-projection beyond `PlacePath` for assignment targets.
- Basic blocks or control-flow graphs. HIR is tree-shaped; MIR is graph-shaped.
- Monomorphized generics.
- Memory layout decisions.

## v1.0 → v2.0 Trajectory

Nothing in this design needs to change for:

- Salsa/query-based incremental compilation — every analysis is already side-table-keyed.
- Arena allocation tuning — already arenas.
- Cross-language imports — Symbols are global, modules are language-tagged but otherwise uniform.
- Ownership inference — HIR doesn't commit to ownership; MIR does.
