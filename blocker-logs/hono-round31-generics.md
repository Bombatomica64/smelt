# Round 31, Agent E — generics across instantiation boundaries

Four briefed items. Three landed; item 1's rule, as briefed, is a measured
REGRESSION and is not in the tree — the evidence and the alternative are below.

Every number here was measured from a CLEAN Hono checkout at
`eebdf7be39abf0a872671835ccce0c4f03ea497a` plus `.github/compat/hono/.`, built
with a freshly built full-feature `smelt` and `--manifest-path <absolute>`, then
`cargo check --message-format=short` on `dist-smelt`.

## The table

| code | baseline | item 2 | item 3 | delta |
| --- | ---: | ---: | ---: | ---: |
| E0308 mismatched types | 51 | 51 | 50 | **−1** |
| E0277 trait bound | 24 | 0 | 0 | **−24** |
| E0631 closure signature | 5 | 5 | 5 | 0 |
| E0382 use of moved value | 5 | 5 | 5 | 0 |
| E0425 unresolved name | 2 | 2 | 2 | 0 |
| E0609 no field | 1 | 1 | 1 | 0 |
| E0599 no method | 1 | 0 | 0 | **−1** |
| E0271 associated type | 1 | 1 | 1 | 0 |
| **total** | **90** | **65** | **64** | **−26** |

The baseline column reproduces the round-31 brief's 90 exactly.

## Item 2 — one bound set on every generic item (landed, −25)

A generic item's type parameters carried one of two bound sets depending on
which emitter wrote it. A generated class's inherent / `Clone` / `PartialEq` /
`Default` impls take `Clone + Default + IntoSmeltUnknown + SmeltFromUnknown +
'static`; the inner record's `Default` impl took the derive-equivalent
`T: Default`. The second cannot prove what the first needs the moment a field's
DEFAULT reaches another generated class, because that class's own `Default` is
emitted with the full set. `Hono_1Inner<E, S, BasePath, CurrentPath>` defaults
its `onError`/`notFound` slots to closures RETURNING
`Hono_1<E, S, BasePath, CurrentPath>` — 24 × E0277, four parameters × three
missing bounds × two slots.

The set is now spelled once, as `classes::GENERATED_TYPE_PARAM_BOUNDS`, and read
by the class inherent impls, the interface impls, generic free functions,
generated unions and both `Default` impls. Per-parameter bounds that belong to
ONE parameter's use — `SmeltJsKeyEq` for a map key, the generated
`F{n}: Fn(..) + ?Sized` callback bounds — are still appended by the caller,
because they are not properties of "being a generated type parameter".

**A callable slot is never a delegation of its own field type.** `Default`'s
`where` clause names the field types whose default delegates, found by searching
the default TEXT for `default()`. A callable field's default is a constructed
no-op closure whose BODY delegates, so the search saw a delegation and demanded
`Rc<dyn Fn(..)>: Default` — which nothing implements, so the whole impl became
unusable and every `Inner::default()` reported E0599. That was Hono's single
E0599, and it had been in the table since the class was first emitted.

Found by the SUITES rather than by Hono: a generated UNION's `Default` impl had
the same defect. Its body constructs the first member's default, and a member is
routinely another generated type — `type Result<T> = Ok<T> | Err<T>` defaults to
`Ok<T>::default()`, which `T: Default` cannot prove. Fixed the same way.

## Item 3 — a substituted type parameter keeps the declaration's ABI (landed, −1)

Round 30, Agent C's leftover. `param_type_is_by_shared_reference` answers TRUE
for `Type::TypeParam`, so a callable slot declared `(value: T) => ..` is emitted
as `Rc<dyn Fn(.., &T)>`; the ABI has to be the CALLEE's, and only its own
declaration knows it. Rust instantiates that slot at `Fn(.., &(String, f64))`
for a `Router<[unknown, RouterRoute]>`, while MIR hands the call site the
SUBSTITUTED parameter — a tuple, which the same predicate answers FALSE for on
its own perfectly correct terms. The argument was packed by value against a slot
spelled `&`.

The rule: **substituting a type parameter at a call site does not change how the
callee is called.** The value is still rendered at the substituted type — that is
what it has to be coerced to — and only the ABI question is asked of the
declaration. `class_field_declared_function_type` answers it for a callee operand
that reads a nominal type's function-typed slot, resolving through `mir.classes`
and then `mir.interfaces` (a `Type::Class` names a generated class OR an
interface record; both carry callable slots and both get instantiated). It is the
sibling of the existing `emitted_call_result_function_type`, and both argument
ladders consult it.

Three alternatives were considered and rejected before this one:

* **Flip `Type::TypeParam` to by-value.** The one-line version. It reintroduces
  the disagreement in the other direction, which the predicate's own docstring
  states: where `T` erases at the caller, the slot's Rust type is `SmeltUnknown`,
  which takes `&`, and the declaration's `Fn(T)` does not. One Rust type, two
  ABIs.
* **Carry the mode on `FunctionType`.** The most general statement, and the
  brief's first suggestion. `FunctionType` is built from 101 struct literals
  across 39 files, none of which uses `..Default::default()`, so the field is a
  whole-tree mechanical change. Worth doing if a shape appears that the
  declaration lookup cannot reach; nothing in Hono or the three corpora does
  today.
* **A side table on `TypeInterner` keyed by `TypeId`.** Additive and it travels
  HIR → MIR, but interning DEDUPLICATES: a substituted `Fn(f64)` is the same
  `TypeId` as a genuinely by-value `(v: number) => ..`, so the table would move
  every such signature to `&`. Consistent, but a corpus-wide ABI change for one
  error.

### FOUND AND NOT FIXED — constructing an interface record at a substituted ABI

The mirror of item 3, on the construction side rather than the call side. An
object literal assigned to `Store<[string, number]>`:

```ts
interface Store<T> { add: (label: string, value: T) => void; size: () => number }
const store: Store<[string, number]> = {
  add: (label: string, value: [string, number]) => { entries.push(value); },
  size: () => entries.length,
};
```

```
expected `Rc<dyn Fn(String, &(String, f64))>`,
   found `Rc<dyn Fn(String, (String, f64))>`
```

The record's slot is `Fn(String, &T)` instantiated at the tuple, while the
closure the literal builds is rendered at the substituted parameter and comes out
by value. Same rule, different site: the ABI of a callable being STORED into a
declared slot is the slot's, not the substituted type's. It is why fixture 96
covers only the class-field half and the interface half is pinned as emitted text
by `a_generic_interface_slot_is_called_with_its_declared_abi`. Not reached by
Hono, which never builds such a record from a literal.

### ALSO FOUND AND NOT FIXED — a class implementing a generic interface does not convert

`class ListStore<T> implements Store<T>` passed where `Store<T>` is expected:

```
expected `Store<(String, f64)>`, found `ListStore<(String, f64)>`
```

Structural conformance to an interface record has no conversion at the call
boundary. Independent of this round; it is why the first draft of fixture 96 was
reshaped.

## Item 4 — manifest correctness (landed; one half does not reproduce)

**`[sources] exclude` matched against the process directory.** Reproduced, fixed
and pinned. `Path::new("Smelt.toml").parent()` is `Some("")`, not `None`, so the
`.unwrap_or(Path::new("."))` that both exclusion predicates relied on never fired
for a manifest named with no directory component — `smelt build` in a project
root, or `--manifest-path Smelt.toml`. An empty prefix makes `strip_prefix`
SUCCEED while stripping nothing, so a canonicalized dependency path stayed
absolute and no manifest-relative glob could match it. Measured on a two-module
project: the relative spelling emitted `dist/src/notes.rs` for an excluded
module, the absolute one did not.

Both predicates now go through `manifest::exclusion_base` and
`manifest_relative_path`. The manifest directory used to RESOLVE roots, entries
and the output target is deliberately unchanged: a root keeps the spelling its
manifest gave it, because that spelling becomes the lowered module's path and
appears in HIR dumps, generated file names and diagnostics. Canonicalizing it
broke `build_hir_reads_entries_relative_to_manifest`, which is the test that says
so.

**`smelt build` writing `.d.ts`/`.pyi` into the source tree so a second build
lowers its own output: DOES NOT REPRODUCE at this head.** Measured directly, on
the shape round 30 named: a clean Hono checkout built twice, the second build
over the first build's stubs, produces byte-identical output across all 33
modules (`diff -rq` on `dist-smelt/src`, no differences). The same holds for a
two-module project. Both the oxc resolver's extension list and
`manifest_import_candidates` put `.ts`/`.py` ahead of `.d.ts`/`.pyi`, which is
`tsc`'s own resolution order, so a generated stub cannot stand in for the
implementation file beside it; `is_probe_source_file` drops `.d.ts` from probe
discovery, and a `.pyi` has an extension the discovery does not accept.

The round-30 note reports the opposite, and I cannot reproduce it. The likeliest
explanation is that it was the same bug as the exclude one — a `git clean` also
removes the `dist-smelt` the build then regenerates, so "clean before every run"
and "use the absolute manifest path" were tested together. Rather than invent a
fix for something I could not make fail, `building_twice_produces_identical_output`
pins the property, so a future regression is a failing test rather than a lost
measurement cycle. The stubs still land beside the source, which is the
documented LSP feature.

## Item 1 — a generic class's own type parameters inside its own generic scope
## (NOT LANDED — the briefed rule regresses Hono; evidence and alternative)

The briefed rule: "a reference to the class without arguments inside its own body
means the current parameters. The defaults are taken only for parameters the
reference does not and cannot mention."

Implemented literally — at an omitted type-argument position, a type parameter of
the same name that is in lexical scope beats the declaration's default — in
`type_arguments_with_defaults` (the class path) and then also in
`type_argument_substitution` (the alias / interface path, which is where Hono's
`H`, `Handler`, `MiddlewareHandler`, `NotFoundHandler` and `ErrorHandler` go).
Measured at each step, from the same clean checkout:

| | total | E0308 | E0631 |
| --- | ---: | ---: | ---: |
| baseline | 90 | 51 | 5 |
| capture in class references only | **110** | 65 | 11 |
| capture in alias/interface references too | **118** | 66 | 18 |

It is a regression, and the reason is structural rather than a bug in the
implementation. The two spellings that have to agree are not both inside a
generic scope:

* `hono-base.ts` declares its DEFAULT handlers at MODULE scope —
  `const notFoundHandler = (c: Context) => c.text('404 Not Found', 404)`. There
  is no `E` in scope there, and there cannot be: `Context` means
  `Context<any, any, {}>`, which is `Context_1<SmeltUnknown, SmeltUnknown,
  SmeltRecord<..>>`.
* the FIELD those handlers are stored in is `NotFoundHandler<E>`, whose alias
  body is `(c: Context<E>) => ..`, lowered once in the ALIAS's own parameter
  scope where `P` and `I` genuinely are not in scope.

TypeScript accepts the assignment because `any` is assignable in both
directions. Rust has no such rule, so the two instantiations are simply
different types, and the capture makes MORE positions spell `E` while the
module-scope side cannot follow. At the baseline both sides erase and agree;
the capture breaks the agreement in 11 more places than it fixes.

The adapter seam cannot bridge it either: `Context_1` is
`Rc<RefCell<Context_1Inner<E, P, I>>>` and `Context_1Inner` holds `_req:
Option<HonoRequest<P, ..>>`, so a conversion would have to rebuild the inner
record and would lose the `Rc` identity that the shared mutable state depends on.

**The alternative, and I think the right one: elide type parameters nothing
carries.** `Context_1<E, P, I>` uses `I` only in `PhantomData`, and `E` only in
`_not_found_handler: Rc<dyn Fn(Context_1<E, ..>) -> SmeltUnknown>` — that is, in
a position of `Context_1` that is itself only reachable through `E`. A least
fixpoint over "parameter positions that reach a non-phantom field" answers
NEITHER is carried. A team hand-writing this Rust would not declare a parameter
its data never holds, and dropping the two collapses `Context_1<E, SmeltUnknown,
..>` and `Context_1<SmeltUnknown, .., ..>` to one type — which closes the whole
family (20 E0308 + 5 E0631 + 2 `expected SmeltUnknown, found type parameter E`)
without any capture rule at all, and without an erasure. `P` survives the
fixpoint (it reaches `HonoRequest<P, ..>`), so `Context_1<SmeltUnknown, String,
..>` vs `Context_1<SmeltUnknown, .., ..>` — 3 errors — would remain.

It is a change to struct arity, so it ripples through every `impl`, every
`Default` body, every reference and all three corpora's goldens. It wants its own
round and its own measurement, which is why it is written down here rather than
attempted at the end of this one.

## SmeltUnknown

No conversion was introduced in any of the three landed commits. Item 3 REMOVES a
mismatch rather than erasing around it; items 2 and 4 emit no new value code at
all.

| baseline | before | after | delta |
| --- | ---: | ---: | ---: |
| examples (hard invariant) avoidable | 0 | 0 | **+0** |
| es-toolkit (ratchet) avoidable | 31716 | 31716 | **+0** |

The examples baseline is re-snapshot twice, for runtime-prelude and legitimate
growth only: once for the wider `Default` impl headers and fixtures 95, once for
fixture 96. The es-toolkit baseline is EQUAL, so it is not re-snapshot (round 30's
precedent); its runtime prelude reads +5 against the stored snapshot, which does
not block. The remeda baseline is advisory and unchanged.

## Gates

| gate | result |
| --- | --- |
| examples invariant (`--fail-on-regression`) | avoidable **0**, delta +0; re-snapshot for prelude/legitimate growth |
| es-toolkit ratchet (`--fail-on-regression`) | avoidable **31716** vs baseline 31716, **+0**; legitimate +0, prelude +5 |
| remeda generated `cargo test` | **1789 passed / 0 failed** |
| radash generated `cargo test` | **84 passed / 0 failed** |
| `cargo test -p smelt-codegen-rust` | 1064 passed / 0 failed, every tier file ok |
| `cargo test -p smelt-transpiler` | 57 + 15 + 17 + 27 + 3 passed / 0 failed, incl. the 89-fixture end-to-end suite |
| `cargo test -p smelt-frontend-ts --no-default-features` | 1092 passed / 0 failed |
| `cargo clippy --all-targets` | no new findings; the one `type_complexity` ERROR the new test helper introduced is fixed with a type alias |

Corpora were rebuilt from clean checkouts with the final binary: es-toolkit
`e008a2818cd8d07469a5cc12ee0c02405d523e07`, remeda
`3c80f28bb394edbf89f1fc9978571dec8ed20edc`, radash
`4cab1900d08e0997abc4f17aec3cbfe18958d766`.

## Fixtures and tests added

| test | what it pins |
| --- | --- |
| fixture `95_generic_class_default_bounds` | a generic class whose callable field returns the class, and a generic class that stores another — both `Default` delegation paths, against Node 22 |
| fixture `96_substituted_parameter_keeps_its_abi` | a generic class's function-typed field called at a scalar and at a heap instantiation, against Node 22 |
| `generic_class_default_impl_carries_the_generated_bound_set` | the inner record's `Default` takes the crate bound set |
| `a_callable_field_is_never_a_delegation_of_its_own_type` | no `Rc<dyn Fn(..)>: Default` in the `where` clause |
| `a_generic_interface_slot_is_called_with_its_declared_abi` | the interface half of item 3, as emitted text |
| `exclude_globs_resolve_against_the_manifest_directory_not_the_process_directory` | three manifest-path spellings, two working directories, one answer |
| `building_twice_produces_identical_output` | a build is a function of the SOURCE, not of the stubs the last build left beside it |
| `a_generic_class_spells_its_derived_impls_instead_of_deriving_them` | UPDATED: it asserted the old derive-equivalent `Default` bound |
