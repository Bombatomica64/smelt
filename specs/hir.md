# HIR (High-Level Intermediate Representation)

The HIR is smelt's first intermediate representation. Both the TypeScript and Python frontends produce HIR. All language-specific concerns end here; everything downstream operates on HIR or its lowered form (MIR).

## Goals

- **Language-agnostic.** Nothing in HIR should look TS-specific or Python-specific.
- **Fully typed.** Every expression node carries a resolved type. There is no inference at the HIR level — that's the frontend's job.
- **High-level enough to be readable.** Classes, methods, exceptions, comprehensions, and async/await are still recognizable. Lowering them is MIR's job.
- **Serde-serializable.** Snapshot tests dump HIR as JSON or RON for diffing.

## Non-Goals

- Optimization. HIR is for *correctness* of language translation, not performance.
- Source-form fidelity. HIR doesn't need to round-trip back to TS/Python.
- Memory efficiency. v1.0 HIR uses `Box`, `Vec`, and `String` liberally. Arena allocation is a future optimization.

## Top-Level Structure

```rust
pub struct Module {
    pub name: ModuleName,           // canonical snake_case name
    pub source: SourceFile,         // path + original language tag
    pub imports: Vec<Import>,
    pub items: Vec<Item>,
}

pub enum Item {
    Function(Function),
    Class(Class),
    TypeAlias(TypeAlias),
    Const(ConstItem),
}
```

## Types

```rust
pub enum Type {
    // Primitives
    Bool,
    Int,        // maps to i64 by default; refined by inference
    Float,      // maps to f64
    String,
    None,       // unit / null

    // Composites
    List(Box<Type>),
    Dict(Box<Type>, Box<Type>),
    Tuple(Vec<Type>),
    Optional(Box<Type>),       // T | null in TS, Optional[T] in Python
    Union(Vec<Type>),          // discriminated unions only

    // User-defined
    Class(ClassRef),
    TypeVar(TypeVarId),        // generics
    Function(FunctionType),

    // Async
    Future(Box<Type>),         // Promise<T> in TS, Awaitable[T] in Python
}
```

Note: there is no `Any`. The frontends reject any code that would need it.

## Expressions

Every `Expr` carries its resolved `Type`:

```rust
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Type,
    pub span: Span,
}

pub enum ExprKind {
    Literal(Literal),
    Var(VarId),
    Call { callee: Box<Expr>, args: Vec<Expr> },
    MethodCall { receiver: Box<Expr>, method: Symbol, args: Vec<Expr> },
    FieldAccess { receiver: Box<Expr>, field: Symbol },
    Index { receiver: Box<Expr>, index: Box<Expr> },
    BinOp { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    UnaryOp { op: UnaryOp, operand: Box<Expr> },
    If { cond: Box<Expr>, then_block: Block, else_block: Option<Block> },
    Match { scrutinee: Box<Expr>, arms: Vec<MatchArm> },  // for discriminated unions
    Lambda { params: Vec<Param>, body: Box<Expr> },
    Await(Box<Expr>),
    ListLit(Vec<Expr>),
    DictLit(Vec<(Expr, Expr)>),
    TupleLit(Vec<Expr>),
}
```

## Statements

```rust
pub enum Stmt {
    Let { name: VarId, ty: Type, value: Expr },
    Assign { target: AssignTarget, value: Expr },
    Expr(Expr),
    Return(Option<Expr>),
    If { cond: Expr, then_block: Block, else_block: Option<Block> },
    While { cond: Expr, body: Block },
    For { var: VarId, iter: Expr, body: Block },
    Try { body: Block, handlers: Vec<ExceptionHandler>, finally: Option<Block> },
    Throw(Expr),
}
```

Note that exceptions are still present in HIR. They get lowered to `Result<T, E>` during HIR → MIR.

## Functions and Classes

```rust
pub struct Function {
    pub name: Symbol,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub is_async: bool,
    pub body: Block,
    pub generics: Vec<TypeVarId>,
}

pub struct Class {
    pub name: Symbol,
    pub fields: Vec<Field>,
    pub methods: Vec<Function>,
    pub generics: Vec<TypeVarId>,
    pub bases: Vec<ClassRef>,   // limited support; see milestone notes
}
```

## Naming Normalization

The HIR canonical form is `snake_case`. The TypeScript frontend converts `camelCase` identifiers at the boundary. Original names are preserved in a side table for error messages so the user sees their own names in diagnostics.

## What HIR Does Not Have

- Lifetimes or borrows. These don't appear until MIR (and even then, naively in v1.0).
- Basic blocks or control-flow graphs. HIR is tree-shaped; MIR is graph-shaped.
- Monomorphized generics. HIR keeps `TypeVar`s as-is.
- Memory layout decisions.

## Validation

The `smelt-hir` crate provides a validator pass that checks invariants:

- Every `Expr` has a non-`None` resolved type.
- Every `VarId` referenced is defined in scope.
- Every `ClassRef` resolves to a known class.
- No `Type::Union` contains `Any` (it doesn't exist, but check anyway).
- Async/await consistency: `Await` only inside `is_async: true` functions.

Validator failures are bugs in the frontend, not user errors. They should crash loudly during development.

## Open Questions (resolve before M2 ships)

- Should `Symbol` be interned globally or per-module?
- How do we represent decorators? (Probably: lower them in the frontend, don't surface them in HIR.)
- How do we represent context managers (`with` in Python)? Likely a desugaring to try/finally before HIR even sees them.
