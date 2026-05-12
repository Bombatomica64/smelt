# Never Type Plan

## Goal

Smelt should accept TypeScript `never` where it is a type-system proof or constraint, while still
preserving Rust runtime safety. `never` must not become an unchecked dynamic escape hatch and must
not invent values that TypeScript says cannot exist.

The immediate compatibility target is Remeda's:

```ts
export type StrictFunction = (...args: never) => unknown;
```

That shape is type-level function variance machinery. It should not generate runtime checks or a
runtime `never` value.

## Semantic Model

TypeScript `never` is the bottom type: no value can have it. Smelt should lower it according to
where it appears:

| Source position | Smelt meaning |
|---|---|
| Function return type `(): never` | Diverging function. The body must not complete normally. |
| Function parameter type `(value: never) => T` | Uncallable parameter contract at source level. Treat as type-only unless the function is called. |
| Rest parameter type `(...args: never) => T` | Opaque variadic function type used for assignability. Erase argument contract for v1. |
| Tuple element type `[never]` | Impossible tuple element. Type-only unless a runtime value is constructed. |
| Tuple rest type `[...never[]]` / `...never` | Empty/impossible variadic type-level tail. Type-only unless a runtime value is constructed. |
| Union member `T | never` | Normalize to `T`. |
| Generic constraint or conditional helper | Type-level only unless it reaches executable value shape. |
| Variable/local/value annotation `const x: never = ...` | Allowed only for expressions that never return; otherwise reject. |

## Runtime Policy

There is no generated Rust runtime value for `never`.

- Do not emit a `SmeltNever` carrier.
- Do not emit runtime guards for `never`.
- Do not convert `never` to `Unknown` in executable value positions.
- Do not allow code to construct, store, inspect, or pass a concrete `never` value.

When `never` is only part of an annotation that constrains assignability, Smelt may erase it after
recording enough shape metadata to keep the surrounding type useful.

## Function Types

For function type annotations, `never` parameter positions are contravariant and often appear as a
library typing trick. Smelt should avoid treating them like required runtime inputs.

Lower this:

```ts
type StrictFunction = (...args: never) => unknown;
```

as an opaque function type with:

- variadic argument shape accepted syntactically;
- no runtime argument contract generated from `never`;
- return type lowered normally (`unknown` here);
- calls through this type rejected unless the call site has a more specific callable type.

This keeps Remeda-style public API surfaces compilable without making arbitrary calls safe.

Non-rest `never` parameters should follow the same safety rule: the function type may exist, but a
call that must supply a `never` argument is impossible unless the argument expression itself is
diverging.

## Return Types

`(): never` means the function does not return normally.

Implementation should eventually model this as a diverging return type, but v1 can accept it in
type-only positions and reject executable functions whose body can fall through. Existing exception
lowering to `Result` is compatible with this: a body that always throws may lower to a terminating
error path, not to a fabricated value.

## Unions And Normalization

`never` is the identity element for unions:

```ts
T | never => T
never | T => T
never | never => never
```

If a normalized type is still `never` and the type is needed for a runtime value, reject unless the
producer is known to diverge. If it is only needed for type metadata, keep the bottom marker or erase
it according to the surrounding construct.

## Tuples And Rest Types

Tuple and variadic tuple types from libraries such as Neverthrow use `never` to express impossible
or filtered elements. These are usually type-level helpers.

Initial support:

- Allow `never` tuple element types in type aliases and signatures.
- Allow tuple rest types whose element is `never`.
- Erase impossible rest tails to an empty tuple/list tail for metadata purposes.
- Reject construction of a runtime tuple/list value that would require a concrete `never` item.

## HIR Representation

Smelt should add an internal bottom type only if it needs to preserve information across frontend
phases:

```text
Type::Never
```

`Type::Never` is allowed in signatures, aliases, tuple metadata, and function type metadata. It is
not allowed as the concrete type of an executable expression unless the expression is terminating.

If adding `Type::Never` is larger than needed for the first Remeda slice, the frontend may initially
erase `never` in strictly type-only function rest parameter positions. That erasure must be local and
documented by tests so it does not become a general `never -> unknown` rule.

## Codegen Policy

Rust emission should never need to render a value of type `never`.

Allowed renderings:

- erased type-only metadata: no Rust output;
- diverging expression/function: Rust `!` may be used when the surrounding generated code supports
  it;
- impossible callable argument: do not emit a callable wrapper that accepts arbitrary values.

Rejected renderings:

- `never` as `()`;
- `never` as `SmeltUnknown`;
- `never` as an uninhabited enum used in reachable public APIs before call semantics are defined.

`()` is a value and would be unsound for TypeScript `never`.

## First Implementation Slices

1. Remeda function rest parameter slice:
   - Lower `TSFunctionType` rest parameters.
   - Accept `never` as a rest parameter element type.
   - Lower `(...args: never) => unknown` to an opaque function signature.
   - Ensure calls through that opaque signature remain rejected unless a more specific overload is
     selected.

2. Union normalization:
   - Drop `never` from unions with at least one non-never member.
   - Preserve or reject bare `never` according to whether the site is type-only or executable.

3. Neverthrow type-surface slice:
   - Accept `never` inside tuple element types used by aliases/signatures.
   - Accept tuple rest types involving `never`.
   - Keep runtime tuple construction rejected when it would need a `never` value.

4. Diverging return slice:
   - Accept `(): never` for functions that always throw or otherwise terminate.
   - Validate that functions annotated `never` do not complete normally.

## Acceptance Criteria

- Remeda's `StrictFunction = (...args: never) => unknown` no longer blocks manifest checking.
- No generated Rust code fabricates a value for `never`.
- A source expression requiring a concrete `never` value is rejected with a clear diagnostic.
- `T | never` normalizes to `T`.
- Bare executable `never` remains rejected unless the expression is known to diverge.
- Tests cover rest function types, unions, tuple metadata, and rejected runtime construction.

