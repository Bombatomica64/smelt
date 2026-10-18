# Typed JSON entry decoding

Design adapted from Bun's `--aot` (oven-sh/bun#45631): "a value that doesn't
match its type throws a `TypeError` where it enters typed code", and
`Bun.validate<T>` / `Bun.parse<T>`, which derive the check from the TypeScript
type you already wrote (`docs/bundler/executables.mdx`, "Validate data by its
type").

## The rule

TypeScript types `JSON.parse(text)` and `response.json()` as `any`, so a parsed
value enters typed code at the **first declared type it flows into**. Smelt
treats each of these positions as a typed entry boundary:

| Source | Target |
| --- | --- |
| `JSON.parse<T>(s)` | `T` |
| `JSON.parse(s) as T` | `T` |
| `const v: T = JSON.parse(s)` | `T` |
| `f(JSON.parse(s))` with `f(x: T)` | `T` |
| `return JSON.parse(s)` in a function returning `T` | `T` |
| `const v: T = await response.json()` / `await request.json()` (also `as T`, `return`) | `T` |

At a boundary whose `T` has a decodable shape (below), the generated Rust parses
the text and **decodes it strictly into `T`'s concrete Rust type** — the
generated `struct`, the tagged `SmeltUnion…` enum, `SmeltList<…>`, a tuple,
`SmeltRecord<String, …>`, `Option<…>` or a primitive. A mismatch throws a
catchable `TypeError` (`error instanceof TypeError` is `true`) whose message
names what the type expected, what the JSON held and where:

```text
expected number, got string "3" at .visits
expected number | null, got undefined at .discount
expected "fast" | "slow", got string "medium" at .mode
expected number, got string "x" at .shapes[0].side
expected Circle | Square, got object at .shapes[0]
expected [number, number], got array of length 3 at .range
expected Order, got array
```

Malformed text still throws the parse's `SyntaxError`, as before.

A parse with no declared target (`const v = JSON.parse(s)`), or one whose
context is `unknown`/`any`, a bare type parameter or `void`, keeps the dynamic
parse unchanged. Python's `json.loads` is unchanged: it never decodes.

## What is checked

* **Primitives** by JSON kind, without coercion: a `number` must be a JSON
  number (`"3"` is not), likewise `string`, `boolean`, `null`.
* **`int32`** (Bun's `import type { int32 } from "bun"`, stored as a Rust
  `i32`): a whole JSON number from -2^31 to 2^31 - 1, as Bun's validator
  checks it. `1.5`, `2147483648` and `-0` (which an `i32` cannot hold) are
  `expected int32` issues.
* **Literal types** by value: `kind: "circle"`, `mode: "a" | "b"` (directly or
  through a type alias), numeric and boolean literals.
* **Properties**: a missing required property is reported as `got undefined`.
  `p?: T` and `p: T | undefined` may be missing. `null` is accepted only where
  the declared type admits `null` — `p?: T` and `p: T | undefined` reject it,
  `p: T | null` accepts it, following TypeScript.
* **Extra properties are dropped**, as `Bun.validate` drops them: the decoded
  struct has no slot for a property its type does not declare.
* **Arrays** element by element, **tuples** with exact arity,
  **`Record<string, T>`** value by value.
* **Unions**: arms are tried in smelt's canonical union order and the first arm
  that decodes wins. When no arm decodes, the report is the issue of the one arm
  whose literal (discriminant) properties matched, if there is exactly one, and
  otherwise the union itself.
* **Nested and recursive** interfaces, through one decoder function per type.
* **`unknown`** positions are kept as parsed: the source declared them dynamic.

## Not decodable: the existing path is kept

A target that contains any of these keeps the existing dynamic boundary (the
erased JSON value, coerced into the destination as before), and nothing throws:

* functions and interfaces with methods (JSON cannot carry a function);
* classes — a class value has prototype identity and methods JSON cannot supply
  (`Bun.validate` rejects these too);
* generic instantiations (`Box<number>`) and unresolved type parameters;
* `Date`, `Map`, `Set`, symbols, promises, generators.

## Divergences from Node

These are intentional and Bun-aligned:

1. **Mismatching JSON throws.** Node carries a wrong value on silently
   (`double(body.visits)` with `"visits": "3"` computes with the string);
   smelt throws the `TypeError` above at the boundary.
2. **Undeclared properties are gone.** `JSON.stringify` of a decoded value
   differs from Node only by the undeclared keys it no longer has.

For JSON that matches its type the decoded values are Node's, except where
smelt's existing storage already differs: a `null` stored in a `T | null`
property reads back as `undefined`, the pre-existing `Option` storage of
nullish values.

## Known limits

* Object-literal TYPES are stored by smelt's existing lowering as a
  string-keyed dictionary (an inline `customer: { id: number; email: string }`
  is a `Record<string, number | string>`), so their decoder checks the
  dictionary's value type and keeps every key. A property typed by an
  object-literal type ALIAS (`pair: Pair` with `type Pair = { a: number }`) has
  no generated struct at all (it is stored as a dynamic value), so the decoder
  passes it through unchecked — the rest of the object is still checked. A
  top-level target that is such an alias keeps the dynamic parse. Declaring the
  shape as an `interface` gets the full check.
* Literal checks apply to a property whose type is a literal union. A literal
  type nested inside an array, record or tuple (`modes: ("a" | "b")[]`) is
  checked by its primitive kind only.
* A top-level `JSON.parse(s) as T | undefined` still admits `null`, because
  the top-level `Optional` carries no source facts.
* `f(await response.json())` (an awaited body read passed straight as an
  argument) is not threaded; bind it to an annotated variable first.

## Where it lives

* Frontend: `json_parse_call_with_hint` and `adopt_json_body_read_target`
  (`crates/smelt-frontend-ts/src/lowering/stdlib.rs`) thread the declared
  target; `lowering/ty/decode_facts.rs` records literal and nullish facts per
  property (`smelt_hir::FieldDecodeFacts`) and per alias
  (`smelt_hir::TypeDecodeFacts`).
* HIR/MIR: `ExprKind::JsonParse { decode }`, `BuiltinFn::JsonParse { target }`,
  `MirInterface::decode_facts`.
* Codegen: `crates/smelt-codegen-rust/src/emitter/json_decode.rs` emits the
  support helpers, one `smelt_json_decode_<T>` entry adapter per target and one
  `smelt_decode_<T>` decoder per composite type, into the generated prelude.
* Tests: `part04_tests.rs` (`json_parse_takes_its_declared_target_from_every_
  entry_position`, `json_body_read_takes_its_declared_target`,
  `interface_records_json_decode_facts_per_property`), `part_4_tests.rs`
  (`emits_strict_json_decoders_for_declared_targets`,
  `keeps_dynamic_json_parse_for_undecodable_targets`), the runtime tier in
  `crates/smelt-codegen-rust/tests/stdlib_boundary_runtime.rs`, and the
  end-to-end example `168_json_typed_entry_decode`.
