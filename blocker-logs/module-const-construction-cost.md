# Module-level `const` construction cost: why the fix is a memoized payload, not a hoisted static

Investigation and fix report for `benchmarks/FINDINGS.md` finding #3 — "module-level
`const` non-primitives are rebuilt at every call site, every call".

## The symptom

es-toolkit declares its word-splitting pattern once, at module scope:

```ts
export const CASE_SPLIT_PATTERN =
  /\p{Lu}?\p{Ll}+|[0-9]+|\p{Lu}+(?!\p{Ll})|\p{Emoji_Presentation}|\p{Extended_Pictographic}|\p{L}+/gu;

export function words(str: string): string[] {
  return Array.from(str.match(CASE_SPLIT_PATTERN) ?? []);
}
```

and the emitted `words` rebuilt the RegExp per call:

```rust
pub(crate) fn words_150(str: String) -> SmeltList<String> {
    let _smelt_tmp_1: SmeltRegExp = SmeltRegExp::new("\\p{Lu}?\\p{Ll}+|…".to_owned(), "gu".to_owned());
```

`SmeltRegExp::new` is cheap; what was expensive is that every *use* of the value then
called `try_compiled()`, which ran `fancy_regex::Regex::new` on the pattern text. One
benchmark op is 5,000 strings, so one op was 5,000 Unicode-property regex compiles.
`camel_case`/`kebab_case` were the worst rows in both libraries.

## Why the const is inlined at all (existing behaviour, unchanged)

`Item::Const` is a **frontend-only** concept. `ModuleBuilder::const_item_expression`
(`crates/smelt-frontend-ts/src/lowering/expr/references.rs`) clones the const's
initializer expression tree into every referencing body via
`clone_const_body_expr`, so MIR and codegen never see a const item — only the pasted
initializer. `blocker-logs/estk-const-item-inlining.md` records why: inlining is what
lets a const initializer of *any* expression shape cross a module boundary without the
type having to be nameable at module scope, and it is deliberately structural
(`ExprKind::try_map_child_exprs`, exhaustive) rather than a whitelist. Only `Local` and
`Block` are rejected, because they point into the source body's arenas.

Nothing here changes that.

## Why "hoist the const into a shared lazily-initialized static" is not sound

The obvious fix — build the initializer once into a `thread_local` `OnceCell` and hand
each use a `.clone()` — is what the runtime prelude already does for function-item
values (`__smelt_fn_value_*`, `__smelt_fn_erased_*` in
`crates/smelt-codegen-rust/src/lib.rs`). For those it is *correct precisely because*
sharing is the goal: JavaScript function singletons must stay `===`.

For data it is the opposite. Every non-primitive value in the generated runtime carries
a JavaScript reference identity, and `Clone` **preserves** it:

```rust
pub struct SmeltList<T> { id: usize, values: Vec<T> }
impl<T: Clone> Clone for SmeltList<T> {
    fn clone(&self) -> Self { Self { id: self.id, values: self.values.clone() } }
}
```

(`fresh_copy()` is the separate seam that mints a new identity — that is what `[...a]`
and `slice` emit.) `SmeltRegExp` is worse than a list:

```rust
pub struct SmeltRegExp {
    id: usize,
    source: String,
    flags: String,
    last_index: ::std::rc::Rc<::std::cell::RefCell<usize>>,
}
```

`last_index` is JavaScript's `lastIndex` — **observable and mutable** (generated code
both reads and writes it; see `regexp_last_index_write_targets_borrow_mut`), and it is
behind an `Rc`, so a clone *shares the slot*. Handing every use site a clone of one
cached instance would therefore:

* fuse the identity of values the source treats as distinct objects, and
* let one call site's `/g` or `/y` scan position leak into another's.

A codegen-level hoist can only key the cache on the *rendered initializer text*, since
the const provenance is already gone by then — so two textually identical regex
literals from two different modules (two distinct objects in JavaScript, with
independent `lastIndex`) would collapse into one. That is a real behaviour change, and
`words`-style code is only accidentally immune to it because
`String.prototype.match` with `/g` ignores `lastIndex`.

Restricting the hoist to *identity-free* types instead is sound but nearly empty: in
this runtime the set of values that are both costly to build and identity-free is
essentially just derived `String`s, and none of the measured regressions are in it.

## The rule chosen

> **Split a construction into its identity-bearing mutable shell and its pure derived
> payload. The shell is built at every use, so reference identity and mutable
> per-object state stay per-use. The payload — a value that is a pure function of the
> literal inputs alone — is built once per distinct input and memoized per thread.**

The rule is stated over the *shape* of the initializer (pure function of its literal
inputs, no observable state), not over any library or any spelling. It is the same
trade the prelude already makes for function items, with the sharing moved to the half
where sharing is unobservable.

Applied to the one prelude type that currently has such a payload, `SmeltRegExp`:

* `SmeltRegExp::new(source, flags)` — unchanged, still per use site. Fresh `id`, fresh
  `last_index`.
* `try_compiled()` now returns `Option<Rc<fancy_regex::Regex>>` and reads through a
  `thread_local` `SMELT_REGEX_CACHE: RefCell<HashMap<String, Option<Rc<Regex>>>>`
  keyed by the *translated* pattern (flag prefix + `[^]` rewrite). The compiled
  automaton depends on the pattern text and nothing else, so sharing it is
  unobservable. `None` is cached too, so an invalid pattern is not recompiled either.
* `thread_local` (not `LazyLock`/`OnceLock`) matches the rest of the single-threaded
  `Rc`/`RefCell` runtime and keeps each `#[test]` thread independent, as the other
  prelude caches do.

The memo is unbounded by design: it retains one automaton per distinct pattern the
thread has built, i.e. one entry per regex literal in the program — the same set the
source itself keeps alive at module scope. A program that builds regexes from *dynamic*
strings (`new RegExp(input)`) would instead retain one entry per distinct input, which
JavaScript would not; that is the shape an eviction policy would be for, and no
arbitrary cap is imposed before there is a corpus case to size it against.

Lists, dicts and objects get nothing from this rule because they have no pure derived
payload: their cost *is* the identity-bearing allocation. Making those construct-once
requires the sound-but-larger change — a real MIR-level module-const item, eagerly
initialized at module init (JavaScript's evaluation order), whose references mint a
fresh identity where identity is observable. That is not done here.

## Effect

Checksums unchanged in every case (the benchmark harness's correctness proof).

| case | before (ops/s) | after (ops/s) | factor | checksum |
| --- | ---: | ---: | ---: | ---: |
| es-toolkit `camel_case` | 0.171 | 66.95 | 391x | 2059234735 |
| es-toolkit `kebab_case` | 0.166 | 83.51 | 503x | 238227297 |
| remeda `camel_case` | 0.093 | 4.47 | 48x | 1652614127 |
| remeda `kebab_case` | 0.097 | 4.79 | 50x | 238227297 |

remeda keeps a larger residual gap; that is finding #1 (whole-collection clones), not
this one.

## Regression tests

* `smelt-codegen-rust::tests::part_7_tests::module_level_regex_const_compiles_its_pattern_once`
  — golden text. A module-level regex const referenced from two functions must yield
  **exactly one** `fancy_regex::Regex::new` call site in the whole emitted crate (the
  memo), while `SmeltRegExp::new` still appears at each use site.
* `smelt-codegen-rust --test regexp_compile_cache_runtime` (`#[ignore]` tier, compiles
  and runs a generated crate) — `lastIndex` advances across successive `exec` calls on
  one object, two objects sharing a pattern keep independent `lastIndex`, and a written
  `lastIndex` still steers the next scan. This is the test that would fail if the
  compiled automaton had been shared by sharing the wrapper.

## Gates

* `cargo clippy --all-targets`: 16 `error` lines, all pre-existing in untouched files
  (identical with the change stashed).
* `cargo test --lib`: 0 failed.
* remeda: 1789 passed / 0 failed.
* es-toolkit: 950 passed / 109 failed (unchanged).
* Both `smelt-unknown-report` ratchets pass; avoidable erasure +0.
