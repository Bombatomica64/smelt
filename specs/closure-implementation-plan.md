# Closure Implementation Plan

## Goal

Add first-class closure support for TypeScript and Python and emit native Rust closures where practical.

This is broader than TypeScript array callback support. The implementation must support closure values flowing through locals, function parameters, callback APIs, and nested scopes, with captures represented explicitly in HIR/MIR instead of hidden frontend state.

## Non-Goals For The First Slice

- No full JavaScript dynamic `this` binding.
- No Python generator closures.
- No recursive anonymous closures unless represented through named functions first.
- No async closures until the sync closure path is green.
- No trait-object heavy design unless direct closure generics cannot represent the needed codegen shape.

## Required Semantics

### Shared

- Closure values have:
  - parameter list
  - return type
  - body
  - capture list
  - capture mode
  - source span
- Captures must be explicit and typed.
- Captures must distinguish:
  - immutable by-reference or cloned capture
  - mutable capture
  - moved capture
- Closure calls must type-check argument count and argument types.
- Closures can be passed as callback arguments.
- Closures can be stored in local variables.
- Closures can be returned only after a dedicated representation for returnable closures exists.
- Unsupported closure forms must produce source-located diagnostics.

### TypeScript

Support first:

```ts
const factor = 2;
const values = [1, 2, 3];
const doubled = values.map((value) => value * factor);
```

Then:

```ts
const offset = 10;
const fn = (value: number): number => value + offset;
const result = values.map(fn);
```

Required callback parameter forms:

- `value`
- `value, index`
- `value, index, array`

Supported callback APIs:

- `Array.prototype.map`
- `filter`
- `find`
- `findIndex`
- `some`
- `every`
- `forEach`
- `reduce` with explicit initial value

Later TS forms:

- nested function declarations
- function expressions
- closure values passed to user functions
- closure return values

Explicitly reject initially:

- `this` inside closure bodies
- rest parameters
- destructured closure parameters
- generic closures
- async closures
- callbacks relying on JS `arguments`

### Python

Support first:

```py
factor = 2
values = [1, 2, 3]
result = list(map(lambda value: value * factor, values))
```

Then:

```py
def scale(value: int) -> int:
    return value * factor
```

Required Python forms:

- `lambda value: expr`
- nested `def` with explicit annotations
- passing a local closure value to supported builtins/APIs

Supported callback consumers:

- `map`
- `filter`
- `sorted(key=...)` once key callbacks are represented
- list-style callback helpers added for Python-only tests if needed

Explicitly reject initially:

- closures over dynamically typed values without annotations
- default args used as capture hacks
- varargs/kwargs closures
- generators
- async closures

## HIR Design

Add closure-capable HIR nodes.

```rust
pub enum ExprKind {
    Closure(ClosureExpr),
    ClosureCall {
        callee: ExprId,
        args: Vec<ExprId>,
    },
}
```

Add:

```rust
pub struct ClosureExpr {
    pub params: Vec<Param>,
    pub return_ty: TypeId,
    pub captures: Vec<ClosureCapture>,
    pub body: BodyId,
    pub span: Span,
}

pub struct ClosureCapture {
    pub source_local: LocalId,
    pub symbol: Symbol,
    pub ty: TypeId,
    pub mode: CaptureMode,
}

pub enum CaptureMode {
    ByRef,
    ByMut,
    ByValue,
}
```

Update `Type`:

```rust
Type::Function(FunctionType)
```

can continue to represent callable signatures, but closure values need either:

- `Type::Closure(FunctionType)` if closure identity must be distinct
- or `Type::Function(FunctionType)` if the compiler can preserve closure metadata on the expression

Decision: start with `Type::Function(FunctionType)` and keep closure identity on `ExprKind::Closure`. Add `Type::Closure` only if MIR/codegen needs it.

Current `CallbackExpr` is too narrow. Replace or bridge it:

- short term: keep `CallbackExpr` for old array callbacks and lower new closures into the same Rust paths only when no captures exist
- target: migrate list callback HIR to accept `ExprId` closure values instead of `CallbackExpr`

Preferred target:

```rust
ExprKind::ListCallback {
    op: ListCallbackOp,
    list: ExprId,
    callback: ExprId,
}
```

where `callback` is an `ExprKind::Closure` or local function value.

## MIR Design

Add MIR representation for closure construction and calls.

```rust
pub enum Rvalue {
    Closure {
        id: ClosureId,
        captures: Vec<Operand>,
    },
    ClosureCall {
        callee: Operand,
        args: Vec<Operand>,
    },
}
```

Add MIR closure table:

```rust
pub struct Mir {
    pub closures: Vec<MirClosure>,
}

pub struct MirClosure {
    pub id: ClosureId,
    pub params: Vec<LocalDecl>,
    pub captures: Vec<MirClosureCapture>,
    pub return_ty: TypeId,
    pub blocks: Vec<BasicBlock>,
}
```

Open design choice:

- Inline closure bodies into codegen-only expression trees for simple callbacks.
- Or lower closures as MIR functions with explicit environment parameters.

Decision: lower closures as MIR functions with explicit environment parameters first. It is easier to validate, optimize, and emit predictably. Codegen can still render Rust closures at call sites when the closure does not escape.

## Rust Codegen Strategy

Use direct Rust closures for non-escaping closures:

```rust
let factor: f64 = 2.0;
let doubled: Vec<f64> = values.iter().map(|value| {
    let value = (*value).clone();
    value * factor
}).collect();
```

For closure values stored in locals but not escaping:

```rust
let scale = |value: f64| -> f64 { value * factor };
let doubled = values.iter().map(scale).collect::<Vec<_>>();
```

For closures that must cross function boundaries:

- Prefer generic function parameters:

```rust
fn apply<F: Fn(f64) -> f64>(value: f64, f: F) -> f64 {
    f(value)
}
```

- Use `Box<dyn Fn(...) -> ...>` only when generic monomorphization cannot be represented in the current codegen structure.

Capture mode mapping:

- immutable read capture: Rust closure captures by shared reference where possible
- moved capture: emit `move |...|`
- mutable capture: require `FnMut`, a mutable closure binding where needed, and a mutable captured local/environment slot

Mutable captures are required in the initial closure design. They are too common in real TypeScript
and Python callback code to defer behind a separate late phase.

## Frontend Implementation

### TypeScript Frontend

Add a lexical scope model for closure lowering:

- current locals
- outer locals
- closure params
- captured locals

During closure body lowering:

- local identifiers resolve to closure params first
- then closure-local bindings
- then outer locals, recorded as captures
- then module items/imports
- assignment to an outer `let` local records a mutable capture
- assignment to an outer `const` local is rejected
- increment/decrement and compound assignment on an outer local record a mutable capture

For array callback methods:

- accept inline arrow closures with captures
- accept local closure values
- accept callback params `(value)`, `(value, index)`, `(value, index, array)`
- reject unsupported callback arities only after checking method-specific max arity

### Python Frontend

Add lambda and nested-def lowering:

- `lambda` becomes `ExprKind::Closure`
- nested `def` creates a local closure value unless it must remain a named item
- outer-scope reads become captures
- assignment to an outer scope through `nonlocal` records a mutable capture
- mutation of captured containers can stay normal method/place mutation when the captured value is mutable
- rebinding an outer name without `nonlocal` follows Python scoping rules and creates a local binding

For Python builtins:

- `map(fn, list)` lowers to list callback operation or general closure call loop
- `filter(fn, list)` lowers to bool callback filtering
- `sorted(values, key=fn)` waits until keyed sort is added

## Validation

HIR validation must check:

- closure capture locals exist
- closure body uses only params, locals, and declared captures
- closure return type matches body return
- closure calls use callable types
- callback consumers receive compatible closure signatures
- mutable captures only target mutable locals or explicit Python `nonlocal` bindings

MIR validation must check:

- closure capture operands exist and match expected types
- mutable closure captures have a mutable storage path
- closure call arguments match parameter types
- returned closure values are rejected until supported

## Tests

### TypeScript Frontend

- inline callback captures immutable outer local
- callback receives `index`
- callback receives `array`
- callback local value passed to `map`
- `reduce` captures outer value
- mutable captured assignment in a callback updates the outer `let`
- assignment to captured `const` rejects
- `this` inside closure rejects

### Python Frontend

- lambda captures immutable outer local
- nested def captures immutable outer local
- local lambda passed to `map`
- lambda passed to `filter`
- nested def with `nonlocal` mutates an outer captured binding
- lambda can mutate captured containers through supported methods
- rebinding an outer name without `nonlocal` is local, matching Python scoping

### MIR

- closure construction rvalue validates captures
- mutable capture rvalue validates mutable environment storage
- closure call validates arg and return types
- callback consumer lowers to closure call path

### Rust Codegen

- emits Rust closure with immutable capture
- emits Rust `FnMut` closure with mutable capture
- emits callback with index parameter using `.enumerate()`
- emits callback with array parameter using local cloned array reference/value
- emits local closure value passed to iterator method
- rejects escaping closure if unsupported

### End-To-End Fixtures

TypeScript:

```ts
const factor = 3;
const values = [1, 2, 3];
console.log(values.map((value, index) => value * factor + index).join(","));
```

Python:

```py
factor: int = 3
values: list[int] = [1, 2, 3]
scaled: list[int] = list(map(lambda value: value * factor, values))
print(",".join([str(value) for value in scaled]))
```

The Python fixture may wait until list comprehensions/string conversion coverage is ready; use frontend/codegen tests first if needed.

## Rollout Slices

### Slice 1: TypeScript Captured Array Callbacks

- Extend callback representation to include captured local reads.
- Support immutable and mutable captures for inline arrow callbacks.
- Support `value`, `index`, `array` callback parameters.
- Emit `FnMut`-compatible Rust closures when the callback mutates captured state.
- Update codegen for `.enumerate()` and array-param closures.
- Keep local callback values unsupported in this slice.

Acceptance:

- frontend tests pass
- codegen tests pass
- `cargo test`, `cargo check`, `cargo clippy`

### Slice 2: TypeScript Closure Values

- Lower arrow/function expressions to closure values.
- Store closure values in local variables.
- Pass local closure values to array callback methods.
- Reject closure return/escaping cases explicitly.

### Slice 3: Shared HIR/MIR Closure Table

- Introduce `ClosureExpr`, `ClosureCapture`, MIR closure table, and closure calls.
- Include `CaptureMode::ByMut` support in the table from the start.
- Migrate TypeScript callback lowering from `CallbackExpr` to closure values.

This slice may happen before Slice 2 if local callback values cannot be represented cleanly with a bridge.

### Slice 4: Python Lambda Captures

- Lower `lambda` to closure values.
- Support immutable captures and captured container mutation.
- Add `map` and `filter` lowering for list inputs.

### Slice 5: Python Nested Defs

- Lower nested `def` with annotations.
- Support passing nested defs as callback values.
- Support `nonlocal` for annotated outer locals as mutable captures.
- Reject varargs, kwargs, and dynamically typed captured rebinding.

### Slice 6: Escaping Closures And Callable Parameters

- Add generic callable function parameters where closures cross user function boundaries.
- Add boxed callable fallback only where generic codegen cannot represent the shape.
- Add tests for closure values passed into user-defined functions.

## Documentation Updates

When implementation starts, update:

- `specs/hir.md`
- `specs/mir.md`
- `specs/frontend-ts.md`
- `specs/frontend-py.md`
- `IMPLEMENTATION_CHECKLIST.md`
- `Test-TODO.md`

## Phase Exit Criteria

- TypeScript captured callbacks work for `map/filter/find/findIndex/some/every/forEach/reduce`.
- TypeScript local closure callback values work.
- TypeScript mutable captures in inline callbacks work for common `let`-backed counters/accumulators.
- Python lambda closures with immutable captures work for `map/filter`.
- Python nested defs with immutable and `nonlocal` mutable captures work.
- Unsupported escaping/dynamic closure cases produce targeted diagnostics.
- Generated Rust uses native closures or clear generic callable parameters, not a custom runtime.
- `cargo test`, `cargo check`, and `cargo clippy` pass.
