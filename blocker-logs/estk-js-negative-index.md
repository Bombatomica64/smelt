# A JavaScript `arr[-1]` is not `arr[arr.length - 1]`

es-toolkit **1014 / 45**, from 1013/46. One row moved
(`at should return 'undefined' for nonexistent keys`), **zero new failures**.

## The defect

`normalized_index_text_with_fallback` in `crates/smelt-codegen-rust/src/emitter/place.rs`
emitted one index normalization for every element access in every language:

```rust
let len = arr.len() as i64;
let index = i as i64;
let normalized = if index < 0 { len + index } else { index };
```

That is **Python's** rule. In JavaScript a negative subscript is a *property
key*, not a position: `['a','b','c'][-1]` is `undefined`, and only
`Array.prototype.at(-1)` counts back from the end. So generated TypeScript
answered `arr[-1]` with the LAST ELEMENT — a plausible wrong value, no panic, no
diagnostic. The helper's own doc comment already claimed the right behaviour
("`arr[-1]` and `arr[arr.length]` are `undefined`"); only the still-negative
case after the wrap actually reached it.

`src/array/at.ts` is the real-world instance. It normalizes negative indices
itself and then reads:

```ts
if (index < 0) { index += length; }
result[i] = arr[index];
```

`at(['a','b','c'], [2, 4, 0, -4])` reaches `-4 + 3 = -1`, which JavaScript
misses. Smelt wrapped a second time to slot `2` and answered `'c'` where the
spec expects `undefined`.

## Why this needed an IR change rather than an emitter switch

The rule is a property of the **source language of the site**, and a crate is
not single-language: `lower_manifest_source` picks a frontend per file via
`SourceLang::from_path`, so TypeScript and Python modules can sit in one
`smelt_hir::Crate`. Nothing downstream of HIR carried the language — not `Mir`,
not `MirFunction::origin`, not `Place`. Codegen could only guess, and both
guesses are wrong for half the corpus.

So the policy is recorded where it is known and read where it is needed:

- `smelt_hir::SourceFile` gains `file: FileId`, so a `FileId -> Language` table
  can be built from the crate's modules. Every span already carries that
  `FileId`.
- `smelt_mir::NegativeIndex` (`FromEnd` / `OutOfRange`) is a new field on
  `Place::Index`, set during HIR lowering from the indexed expression's own span
  (`LoweringCtx::negative_index_policy`).
- The emitter's read and write normalizers take it and skip the `len + index`
  term for `OutOfRange`, letting the existing out-of-range machinery
  (`usize::try_from(..).unwrap_or(usize::MAX)`, then `Vec::get` missing) produce
  the `undefined` both languages already agree on for `arr[arr.length]`.

An expression whose span names no source module (a synthesized lowering) gets
`OutOfRange`: those come from the TypeScript-shaped paths, and it is the choice
that cannot invent a slot the source did not address.

Python is unchanged and guarded by
`a_python_element_read_still_wraps_a_negative_index`, sitting next to the
TypeScript test so the contrast is visible in one place.

## Write path

The write normalizer takes the same policy, so a TypeScript store to a negative
index now hits `usize::try_from(normalized).expect("negative index out of bounds")`
instead of silently overwriting a real element. Neither is what JavaScript does
(it sets a string-keyed property, which Smelt does not model), but the existing
comment on that path already argues the case: "silently redirecting the store to
some other slot would corrupt the collection". No corpus row moved either way.

## Gates

| Gate | Result |
| --- | --- |
| es-toolkit | **1014 / 45** (was 1013 / 46), 1 resolved, **0 new** |
| remeda | 1789 / 0 |
| radash | 84 / 0 |
| workspace `cargo test --all-targets` | green (30 test binaries) |
| `array_index_undefined_runtime` (`--ignored`) | 2 / 2 |
| SmeltUnknown es-toolkit ratchet | +0 / +0 / +0 |
| SmeltUnknown examples invariant | avoidable **0** |
| `cargo clippy --all-targets` | error set identical to `origin/main` |

The new runtime tier was verified in both directions: with the emitter
temporarily restored to the unconditional wrap,
`a_negative_element_read_does_not_count_back_from_the_end` fails on
`read([10, 20, 30], -1)` and `readNested([[1, 2], [3, 4]], 1, -1)`; with the
fix it passes. Its sibling `an_out_of_range_element_read_is_undefined` passes
either way, which is why it did not catch this.

Clippy was compared against a fresh `origin/main` worktree. The main run stops
early (`smelt-python` fails first), so it reports fewer sites; every site this
branch reports that main's run did not reach is in a file **byte-identical to
main** (`git diff origin/main` empty) or in `smelt-specialize`, which was
re-linted on main separately and produced the same five. No error site is in
code this change wrote.

Six goldens moved, all the same single edit — the `len` binding and the
`if index < 0` term dropped from TypeScript element reads. `word.at(-1)` keeps
its wrap in `string_index_and_for_of_emission`, which is the assertion that the
two paths did not merge. Four `examples/` goldens moved for the same reason;
`git diff examples/` is six lines, all of them that shape.

## Left undone — the second `at` row, with its root now measured

`at should return undefined for non-integer indices` still fails, and it is
**not** this defect. PR #246 recorded a lead that the remaining `at` rows might
not be "a typed `SmeltList` cannot hold a hole" because the generated `at.rs` it
read came from `src/compat/object/at.ts`. That was the wrong file. Measured:

- `at_spec.rs` is generated from `src/array/at.spec.ts` and calls `at_0`.
- `at_0` is defined in `at_1.rs`, generated from `src/array/at.ts`.
- `src/compat/object/at.ts` generates `at.rs`, which the spec never calls.

So the original diagnosis was right and the correction was wrong. The failing
assertion is `at(data, indices)` where `data: string[]`, so the call
monomorphizes to `at_0::<String>` returning `SmeltList<String>`. An out-of-range
element read inside a generic function uses `element_missing_value_text`, which
for an in-scope type parameter is `Default::default()` — `""` at `T = String`,
where JavaScript has `undefined`. A hole in a typed list has no representation.

That is a real dynamic boundary worth naming, but it is a different change with
its own blast radius (it touches every generic collection read), so it is left
here rather than folded in.

## A second lead, not yet a diagnosis

`maxBy`/`minBy` `if array is empty, return undefined` fail for a root that is
neither of the above. `max_by_120` is correct on its own — it returns
`None::<T>` for the empty case. The spec's binding is what loses it:

```rust
let result: Person;                       // source says `Person | undefined`
let _smelt_tmp_4: Person = max_by_120(..).clone().map_or(Default::default(), ..);
_smelt_tmp_5 = !(false);                  // `toBeUndefined()` folded statically
```

The `Option<SmeltUnknown>` the call returns is collapsed into a concrete
`Person` by the de-erasure coercion, so `undefined` is unrepresentable at the
binding and `expect(result).toBeUndefined()` folds to a constant `false` before
it can run.

Four minimal repros did **not** reproduce it — with a module-level `interface`,
with a type alias, with a one-argument callback, and with the three-argument
`(element, index, array)` callback that forces `T` to erase. All four emit
`let result: Option<Person>` and the correct `.map(..)` de-erasure. Whatever
selects `map_or(Default::default(), ..)` over `map(..)` here is still unidentified;
the spec declares `type Person` inside the `it` callback, which is the one
difference not yet isolated. Anyone picking this up should start by finding the
coercion site that chooses between those two, not by reading `maxBy`.
