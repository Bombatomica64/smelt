# TS7 parity, phase 2, round 9: instantiated generic calls, element types

This round works on worklist families 1 and 2 of `ts7-parity.md`:
uninstantiated generics at call sites, and erased element types. It also fixes
union-list bugs that typing more literals as unions exposed. Every rule is
covered by example 191, whose output matches Node with zero avoidable
`SmeltUnknown`. Each frontend rule was checked against its defect: with the
rule disabled, example 191's `expected.hir` and `expected.mir` differ.

## 1. Array literals under a still-generic hint (`lowering::open_array_hint`)

`chunk<T>(arr: readonly T[], size)` called as `chunk([1, 2, 3], 2)` lowered the
literal at its contextual hint `List<T>`. That `T` is the callee's, so the
literal had a type no caller value has. HIR type parameters are identified by
name, so inference then matched `T` against `T` and the result kept `T[][]`.
Codegen had nothing to bind and erased the call to `chunk::<SmeltUnknown>`.

Rule: when an array literal adopts a list or tuple hint, each hint slot that
still mentions a type parameter is filled with the type its elements lowered
to. A list slot needs all elements to agree. A tuple fills slot by slot.
Erased or disagreeing elements keep the hint, as does an empty literal. A
caller's own in-scope `T` only reaches an element of type `T`, so the fill is
the identity there.

- Behaviour: unchanged (the same values).
- Types: `chunk::<f64>` returns `SmeltList<SmeltList<f64>>`.

## 2. Evolving arrays (`TsTypeFacts::evolved_array_type_id_at`)

`const kept = []; .. kept.push(item); return kept;` is `T[]` in TypeScript, but
the empty literal has no element evidence and erased. TypeScript reports
`any[]` at the declaration and at mutation receivers (`.push`, and also
`.length`). Only a later plain read shows the evolved type.

The checker bridge (`smelt-ts-types/src/walk.rs`) now records, for each
unannotated `= []` declaration, the evolved type at its last plain read in
source order, when the element is neither `any` nor `never`. The frontend
lowers that type as if it were the annotation (`checker_evolved_array_type`).
It re-spells the type and declines like an annotation does.

## 3. Heterogeneous literals (`checker_mixed_array_item_type`)

`[1, "two", true]` erased (`mixed array literal`), while TypeScript types it
`(string | number | boolean)[]`. The same literal written with that annotation
already lowered to a typed union list. A literal with no hint and disagreeing
elements now takes the checker's element type, unless the literal mixes `null`
and `undefined` (that remains the documented dynamic boundary of
`array_literal_mixes_nullish_spellings`).

To make nested literals (`[1, [2, [3]]]`) typed too, codegen's
`union_member_is_concrete` now accepts a union nested under a container arm
when that union is itself concrete (its own generated enum).

## 4. Union-list bugs these rules exposed (existing on `main` for annotated unions)

These were wrong for `const xs: (number | string)[]` already. Rules 1–3 send
more values down the union path, so they are fixed at their source:

- `includes` on a union list folded to a constant `false`. A needle that is a
  union member is now injected and compared with SameValueZero, as `indexOf`
  does (`list_contains_text`).
- A `typeof` guard on a union-typed callback parameter did not narrow:
  `xs.map((v) => typeof v === "number" ? v + 1 : v + "!")` printed `11`. The
  compact callback IR has no union projection, so such a callback now retries
  through full closure-body lowering (`reject_callback_union_typeof_guard`),
  which narrows like a function declaration does.
- `join` rejected union elements; they now stringify through their runtime arm.
- `flat` on a union element erased the whole value. It now types the result as
  TypeScript's `FlatArray` (`lowering::union_flat`) and flattens through the
  erased runtime helper with typed boundaries on both sides.

## Parity

| corpus | before (main) | after |
| --- | ---: | ---: |
| es-toolkit | 86.8% | 88.9% |
| hono | 93.6% | 93.9% |
| remeda | 91.4% | 91.7% |
| examples | | 96.9% (adds example 191) |

No verdict bucket got worse on any corpus. es-toolkit: agree +600, mismatch
−589, smelt-erased −11. remeda: agree +70, mismatch −70. hono: agree +36,
mismatch −26, smelt-erased −13. The `List<TypeParam> ← number[]/string[]`
families on es-toolkit fell from 424 to 21.

## Ratchets

| | before | after |
| --- | ---: | ---: |
| examples SmeltUnknown avoidable (invariant) | 0 | 0 |
| es-toolkit SmeltUnknown avoidable | 20,858 (main build; baseline file 22,157) | 18,167 (re-snapshotted) |
| examples typedness | | no program less typed; re-snapshotted (adds 191) |
| es-toolkit typedness | | one program less typed, see below; others re-snapshotted |

**`compat/function/overArgs.ts`** (values: erased 25 → 26). On main,
`_transforms.flat()` (a `Many<fn>[]`) was lowered to `none`, so `overArgs`
dropped every transform. It now builds the list. The functions in it are erased
because the source types them `(...args: any[]) => any`, and the `flat`
temporary adds one erased value. `--update-baseline` keeps the old record, so
the blocking ratchet flags this file. That is a behaviour fix, recorded here
for review; the baseline was not hand-edited.

## Found and not fixed

- Rule 1 does not fill a slot from function-typed elements. A generic callee
  whose type parameter is reachable only through callbacks in a container
  (`cbs: ((inner: (v: T) => T) => T)[]`) stays erased in Rust, and the call
  boundary has no adapter from a list of concrete closures to erased ones
  (the codegen shape grid's `g1_nest_vec_*` cases failed to compile).

- `pairUp<A, B>(pair: [A, B])` whose body formats `A`/`B` in a template string
  is still not lifted to real Rust generics by codegen (stringifying a bare
  `T` needs erasure). The call site is now typed `(f64, String)`.
- Calls whose argument is a narrowed `ArrayLike<T> | null` (`toArray(arr)` in
  es-toolkit `compat`) still leave the callee's `T` unbound: worklist family 7
  (flow narrowing), not inference.
- Expected values in matchers (`expect(x).toEqual([1, 2])`) take an erased
  actual's type as their hint and stay erased.
- An evolving array only ever read through `.length` gets no evolved type
  (TypeScript reports `any[]` there).
