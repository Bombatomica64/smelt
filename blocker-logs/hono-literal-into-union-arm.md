# A literal passed into a union parameter is typed by the arm

Round 27, item 4. Measured in round 26 while keeping the examples invariant at
0 for fixture 80; **fixed**. Not a wrong value — a wrong TYPE, which is the
north-star bar: the erased spelling is what a hand-written Rust port would
never produce.

## The measurement

```ts
type Slot = number | [string, number][];
function describe(slot: Slot): string { … }
```

| call | emitted argument, before | after |
| --- | --- | --- |
| `describe([['a', 1], ['b', 2]])` | `SmeltList<SmeltUnknown>` (8 avoidable-erasure lines) | `SmeltUnion4::M1(<SmeltList<(String, f64)>>)` |
| `const pairs: [string, number][] = …; describe(pairs)` | `SmeltUnion4::M1(pairs)` | unchanged |

The two spellings answered the same VALUES; only the types differed.

## Why

`array_expression` reads its contextual hint by matching `Type::Tuple` (for
per-index element hints) and `Type::List` (for a homogeneous element hint). A
UNION hint is neither, so the literal lowered with no hint at all: its elements
inferred independently and erased, and the argument was converted into the
union afterwards.

## The rule

A union hint is projected to the arm the literal can be lowered at, and the
existing union injection at the call boundary wraps the concretely typed value —
which is exactly what the typed-local spelling already did.

Two guards keep it honest:

- **exactly one** list-or-tuple arm. With two (`string[] | number[]`) the
  literal's own elements are the better evidence, which is what the no-hint path
  already uses;
- **no erased arm**. A union containing `unknown` or a type parameter
  (`unknown[] | unknown`) renders as the erased carrier, so its arms are not
  distinguishable in Rust and typing the literal at one buys nothing — the
  literal is erased either way, and the erased spelling is the one that
  round-trips (`emits_list_literal_for_union_destination_as_unknown_array`
  pins this).

## What moved

Two existing goldens improved and their stdout is byte-identical:
`55_headers_init_union` and `65_typeof_keyof_alias_union` now build the inner
`['content-type', 'text/html']` literal as a `(String, String)` tuple instead
of a `SmeltList<String>` that was converted element-by-element afterwards.

## Guarded by

`examples/typescript/end-to-end/85_literal_into_union_arm`: the direct literal,
the typed-local control, the other arm, a NESTED literal (the arm's element type
types the inner literals too), and the ambiguous two-list-arm case that must
keep inferring from its elements. Its program half emits zero
`SmeltList<SmeltUnknown>`, so the examples invariant holds by construction.

## Not covered

An OBJECT literal passed into a union parameter is the same shape and is not
part of this rule (the projection only looks for list/tuple arms). Worth the
same treatment when a corpus shows it; no measurement yet.

## The es-toolkit ratchet: +5, with the accounting

`avoidable-erasure` 32147 -> 32152, re-snapshotted. The shapes show the change
is a container becoming CONCRETE, not new erasure:

| delta | site | shape |
| ---: | --- | --- |
| +4 / -4 | `pick_1.rs:65` | `SmeltList<SmeltUnknown>` built from a `vec![..]` replaces `SmeltUnknown::Array(vec![..])` — a typed container instead of the erased carrier |
| +1 / -1 | `invoke.rs:53`, `result.rs:38`, `has.rs:80` | the same substitution |
| +4 / -4 | `isEqualWith_spec.rs:3108` | `.clone().clone()` collapsed to `.clone()` (round 26's optional-read change), same line reclassified |
| +48 / -48 | `attemptAsync_spec.rs`, `cloneDeepWith_1.rs` | the optional-index arms moved between files, same text |
| +3 | `after_spec.rs:190/257` | two erased-list declarations |

Every one of those is a program whose ELEMENTS were already erased; what
changed is that the container around them is now a `SmeltList` rather than a
tagged `SmeltUnknown::Array`, which is the direction the north star asks for.
The residual +3 is declaration lines in one spec file.
