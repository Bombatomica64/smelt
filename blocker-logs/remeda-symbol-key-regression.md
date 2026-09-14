# remeda regression: a folded symbol key and a runtime-derived one were two keys

Round 10, item 1. Gate: remeda at `3c80f28bb394edbf89f1fc9978571dec8ed20edc` +
`.github/compat/remeda/.`, generated crate `cargo test`.

| | before | after |
| --- | --- | --- |
| remeda generated tests | 1787 passed / **2 failed** | **1789 passed / 0 failed** |

The two failures were `groupByProp` "must be grouped correctly by Symbol",
data-first and data-last.

## What it was

A symbol is a value and a property key at once, so the *key* a symbol names has
to be derived the same way on both sides of the compiler. Round 8 gave the
frontend a second derivation and never gave the runtime the matching half:

* **frontend** (`lowering/ty/computed_key_symbols.rs`): a module-level
  `const SYMBOL = Symbol("sym")` is evaluated once, so it is a stable key, and a
  computed key that reads it folds to a synthetic member name —
  `__smelt_symbol_unique_symbol_sym_141` (sanitized value spelling
  `Symbol(sym)@141`). This is also what makes `class C { get [KEY]() {} }` an
  ordinary member, which is why the key has to be spellable as a Rust
  identifier.
* **runtime** (`smelt_symbol_property_key` in the emitted prelude): knew only
  the well-known table and the generic fallback, so the same symbol arriving as
  a *value* keyed `__smelt_symbol:Symbol(sym)@141`.

remeda's test writes the property through the folded key
(`{ [SYMBOL]: "cat", 2: 123 }` → `SmeltRecord::from([("__smelt_symbol_unique_symbol_sym_141", ...)])`)
and reads it through the value: `groupByProp` is `purry`-wrapped, so `prop`
reaches `groupByPropImplementation` erased and `item?.[prop]` goes through
`smelt_property_key`. The read missed, `key` was `undefined`, every item was
filtered out and the grouping came back empty — **no diagnostic**, a silent
wrong value. Any source of that shape (a symbol-keyed literal plus a
generically-keyed accessor) had the same defect; nothing about it is remeda's.

## The rule

`crates/smelt-stdlib/src/symbol_keys.rs` (new) owns the derivation for the
symbol kinds that are not language constants, next to `well_known_symbols`,
which already owned it for those. `storage_key_for_value_spelling` is total over
the four kinds — well-known, registry `Symbol.for(d)`, unique `Symbol(d)@offset`,
anything else — and both halves consult it: the frontend when it folds a
computed key, and `emit_symbol_key_derivation` (`smelt-codegen-rust/src/lib.rs`)
when it renders `smelt_symbol_property_key` for the generated crate.

Two consequences had to be settled to make one derivation possible:

1. **The key must be invertible.** `Object.getOwnPropertySymbols`,
   `Reflect.ownKeys` and erased-Map enumeration hand a stored key back to the
   program *as a symbol*, and a key that merely sanitized the description
   (lower-cased, punctuation collapsed) cannot be inverted. The folded key now
   uses a reversible identifier encoding — an ASCII alphanumeric stands for
   itself, everything else becomes `_<hex>_`, and `_` itself is escaped, so an
   underscore in the output can only open or close an escape:
   `Symbol(sym)@141` ⇄ `__smelt_symbol_unique_sym_at_141`,
   `Symbol.for(@ts-pattern/matcher)` ⇄
   `__smelt_symbol_for__40_ts_2d_pattern_2f_matcher`. `smelt_own_symbol_key_value`
   is the runtime inverse (opaque + folded forms; deliberately NOT the
   well-known keys, since Smelt also writes `__smelt_symbol_iterator` onto erased
   iterables as part of their representation and reporting that as a
   source-written symbol would invent a property).
2. **Symbol keys never enumerate as string keys.** In JavaScript a symbol key
   appears only in `Object.getOwnPropertySymbols` / `Reflect.ownKeys` — never in
   `Object.keys`, `Object.values`, `Object.entries`, `for...in` or
   `JSON.stringify`. The filters tested `__smelt_symbol:` only, so a folded key
   leaked (and, before this round, a *well-known* key leaked too). Every storage
   spelling shares the `__smelt_symbol` stem, so one prefix test now covers all
   three: `smelt_is_for_in_object_key`, `smelt_is_for_in_record_key` and the 13
   `Object.keys`/`values`/`entries` projections in `emitter/map.rs`.

Interim states worth recording, because each was a real (fixed) defect: deriving
the folded key at run time without (2) broke 11 remeda tests that assert symbol
keys are filtered out of key enumeration, and without (1) broke the three
existing `symbol_key_runtime` fixtures, whose symbols come back out of
`Object.getOwnPropertySymbols`.

## Tests

* `crates/smelt-stdlib/src/symbol_keys.rs` — determinism, identifier-safety, the
  value-spelling↔key round trip for every kind, and escape reversibility
  (including a malformed escape).
* `crates/smelt-codegen-rust/tests/symbol_key_runtime.rs` —
  `a_folded_symbol_key_and_a_runtime_derived_key_name_one_property`: an erased
  read of a folded write, a folded read of an erased write, string-key
  enumeration (`Object.keys` / `Object.values` / `for...in`) excluding both a
  unique and a registry symbol key, `getOwnPropertySymbols` returning a symbol
  that is `toBe`-identical to the original const, and the grouping idiom that
  regressed.
* `smelt-frontend-ts` `class_module_tests` and `computed_key_symbols` now assert
  the folded member name *equals the shared derivation* rather than a hard-coded
  spelling, so the two halves cannot drift again without a test failing.

## Not fixed, found on the way

Inside the regression fixture, `output[k].push(item)` where `output` is an
erased object (`any`) does not write back: the pushed element is lost, while the
same idiom on a typed record (remeda's own `groupByProp`, whose `output` is a
`SmeltRecord<String, SmeltList<_>>`) works. The fixture uses
`output[k] = [...bucket, item]` instead. This is a mutation-through-erased-read
gap, unrelated to symbol keys, and it is not covered by any current family.
