# Unknown Type Plan

## Goal

Smelt should accept TypeScript `unknown` as a safe boundary type so typed libraries can compile
without weakening type safety. `unknown` is not `any`: any value may flow into it, but code may not
inspect or operate on it until Smelt has proof from narrowing or an explicit assertion.

## Model

- Add `Type::Unknown` to HIR and carry it through MIR.
- Lower TypeScript `unknown` to `Type::Unknown`.
- Lower `readonly T` type operators as type-level readonly metadata erasure for v1, preserving the
  inner type. This lets common API surfaces such as `readonly unknown[]` lower as `List<Unknown>`.
- Emit `Type::Unknown` in Rust as a safe generated type, not as an unsafe dynamic hole.
- Use an opaque zero-sized carrier only for signatures that never inspect the value.
- Use a tagged runtime value once code must execute `unknown` narrowing.

## Safety Rules

- `T -> unknown` is allowed.
- Passing, storing, returning, and forwarding `unknown` unchanged is allowed.
- `unknown -> T` is rejected until Smelt has a runtime-safe narrowing or assertion path.
- Method calls, field access, indexing, arithmetic, and non-identity operations on `unknown` remain
  unsupported.
- Containers of `unknown` may exist, but element use still requires narrowing.

## Rust Emission

For library APIs that only pass `unknown` through, generated Rust may emit:

```rust
#[derive(Clone, Debug)]
pub struct SmeltUnknown;
```

and use `SmeltUnknown` wherever a reachable signature contains `unknown`. This is intentionally
opaque. It lets library APIs compile while preventing accidental behavior from being invented.

For executable `unknown` values, especially `JSON.parse`, plugin boundaries, or code that uses
`typeof value`, `Array.isArray(value)`, `value === null`, or user assertion functions, the carrier
must become tagged:

```rust
#[derive(Clone, Debug)]
pub enum SmeltUnknown {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<SmeltUnknown>),
    Object(std::collections::BTreeMap<String, SmeltUnknown>),
}
```

Narrowing then lowers to tag checks plus extraction. This has runtime cost only where a value is
actually typed as `unknown`: one enum tag branch per guard, plus normal allocation cost for dynamic
strings, arrays, and objects. It should not slow normal typed code, because `string`, `number`,
`boolean`, arrays, records, and classes still lower to direct Rust representations.

## Narrowing Implementation Plan

- Add a tagged `SmeltUnknown` runtime representation for executable unknown values.
- Lower `typeof value === "string" | "number" | "boolean" | "undefined" | "object"` to tag checks.
- Lower `Array.isArray(value)` to a tag check when `value` has type `unknown`.
- Lower `value === null` and `value !== null` to null tag checks when `value` has type `unknown`.
- Track guard-derived local narrowings while lowering `if`, `&&`, and assertion calls.
- When a narrowed local is used, emit extraction from the tagged value instead of retyping the same
  Rust variable.
- Support user assertion functions declared as `asserts value is T` by applying the same narrowing
  after a successful call.
- Lower TypeScript `as T`, `<T>value`, `satisfies`, and non-null assertions using TypeScript
  semantics: they are source-level assertions, not proof. For `unknown -> T`, they must become a
  checked extraction from `SmeltUnknown` or a clear runtime panic, not an unchecked Rust cast.
