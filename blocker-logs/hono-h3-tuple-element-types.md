# Hono family H3 — a tuple element that is an ordinary type

Probe: `smelt --manifest-path third_party/hono/Smelt.toml check --message-format json`
at `honojs/hono@eebdf7be39abf0a872671835ccce0c4f03ea497a`.
2 occurrences, 1 file, one shape.

## 1. The sites

`src/types.ts:159` and `src/types.ts:1496`, both the same overload shape — a
rest parameter typed as a tuple whose first element is an intersection:

```ts
...handlers: [H<E2, P, I> & M1, H<E3, P, I2, R>]
```

This is how Hono types "a middleware followed by a handler": `M1` narrows the
middleware's own handler type, so the element is `H<…> & M1`.

## 2. Wrong output

Lowering rejects the file:

```text
tuple element type is not lowered yet: TSIntersectionType(TSIntersectionType { … })
```

with the full AST dump of the intersection in the message — an accurate sign
that the arm was a fallthrough rather than a considered refusal.

## 3. Responsible function

`ModuleBuilder::tuple_element_type_to_hir`,
`crates/smelt-frontend-ts/src/lowering/ty/annotations.rs:1305`.

`TSTupleElement` in oxc is declared as

```rust
pub enum TSTupleElement<'a> {
    TSOptionalType(Box<'a, TSOptionalType<'a>>) = 64,
    TSRestType(Box<'a, TSRestType<'a>>) = 65,
    // `TSType` variants added here by `#[ast]` macro
    INHERIT(TSType<'a>),
}
```

so **every** `TSType` variant is also a `TSTupleElement` variant; only the first
two are tuple-specific. `tuple_element_type_to_hir` was a hand-maintained
re-implementation of `ts_type_to_hir` over that inherited set: it listed
keywords, arrays, nested tuples, type references, type literals, literal types,
indexed access, conditionals, `readonly`/`keyof` operators, parentheses,
function types, unions, and the three tuple-only forms — and ended in

```rust
_ => Err(SmeltError::unsupported(span, format!("tuple element type is not lowered yet: {item:?}"))),
```

Intersections were simply never added. So were `TSTypeQuery` (`typeof x`),
`TSMappedType`, `TSInferType`, `TSImportType`, `TSTypePredicate`, and the rest —
every one of them lowered fine as a type *anywhere else*, and only failed in
tuple position.

## 4. Design

The right statement is structural, not another arm: **a tuple element that is
not one of the three tuple-only forms IS an ordinary type**. The duplicated
subset is a shortcut, so the fallthrough delegates instead of rejecting:

```rust
element => match element.as_ts_type() {
    Some(ts_type) => self.ts_type_to_hir(ts_type),
    None => Err(SmeltError::unsupported(…)),
}
```

`TSTupleElement::as_ts_type()` is oxc's own accessor for the inherited variants,
so the `None` arm is genuinely unreachable for anything but a future
tuple-specific variant, which is exactly what should still be reported.

Two deliberate choices:

* The existing shortcut arms are **kept**, not deleted. Several of them differ
  from `ts_type_to_hir` in ways that matter in tuple position — the nested-tuple
  arm consults `homogeneous_tuple_rest_type` / `tuple_rest_list_type` /
  `leading_rest_tuple_list_type` and drops `never` tails, and the union arm
  collapses list-shaped unions. Replacing the whole match with a delegation
  would have changed all of them. Only the previously-*failing* set moves.
* No erasure is introduced. An intersection in tuple position now lowers through
  the same structural-intersection rule as an intersection anywhere else
  (`ts_type_to_hir`, `annotations.rs:168`), which produces a callable-object
  class or a merged structural type — a concrete type, not `SmeltUnknown`.

## 5. Generality

The rule mentions no library and no type constructor. It fixes intersections in
tuple position because it fixes *every* `TSType` in tuple position; the fix
would have landed identically if the corpus had used `typeof x` or a mapped type
there instead.

## 6. Tests

`crates/smelt-frontend-ts/src/tests/part04_tests.rs` —
`lowers_tuple_elements_that_are_ordinary_types`: a type alias `[Named & Tagged,
Named]` and a rest parameter spelled with the same tuple (the Hono shape), with
both element types read back through indexing so the elements are not merely
parsed but used. Asserts the module lowers and `smelt_hir::validate` is clean.

No runtime tier: the fix is entirely in type lowering and the resulting types
are exercised by the read-back in the frontend test; there is no new emitted
construct to run.

## 7. Result

2 occurrences -> 0. `src/types.ts` leaves the blocker list.
