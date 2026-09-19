# Round 30, Agent C — type lowering

Three items, all landed as general TypeScript-semantics rules. Every number here
was measured from a CLEAN Hono checkout at `eebdf7be39abf0a872671835ccce0c4f03ea497a`
plus `.github/compat/hono/.`, with a freshly built full-feature `smelt`.

## Measuring Hono: two traps

Both cost a measurement cycle, and both are worth writing down before anything
else, because either one silently invalidates a table.

1. **`smelt build` writes into the SOURCE tree.** It emits a `.d.ts` and a
   `.pyi` beside every module it lowers. A second build in the same checkout
   therefore lowers its own output: hono's `src/context.d.ts` (generated,
   non-generic) shadows `src/context.ts` (the real, three-parameter `Context`),
   and the error table drifts. Every run must start from `git clean -xfd`.
2. **`smelt build` with a cwd-relative manifest does not apply
   `[sources] exclude`.** Run from inside the checkout, `smelt build` (manifest
   defaulting to `./Smelt.toml`) lowers `src/client/**` — which the manifest
   excludes — and stops on `string replace requires string-compatible receiver,
   pattern, and replacement`. The same tree with
   `smelt --manifest-path <abs>/Smelt.toml build` builds all 33 modules. Found
   while reproducing the round-30 baseline; NOT investigated further, and not
   caused by anything in this round. The CI recipe happens to use the absolute
   form, which is why it has never shown up there.
   `DependencyCollector::excluded_target` strips `manifest_dir.canonicalize()`
   off a canonicalized dependency path, so the likely cause is the relative
   manifest directory failing that prefix strip; it deserves its own fix.

The measurement recipe used throughout:

```sh
cd <hono>; git clean -xfdq; git checkout -q .
cp -R <worktree>/.github/compat/hono/. .
<worktree>/target-priv/debug/smelt --manifest-path <hono>/Smelt.toml build
cd dist-smelt && CARGO_TARGET_DIR=<scratch> cargo check --message-format=short
```

## The table

| code | baseline | after item 1+2 | after item 3 | delta |
| --- | ---: | ---: | ---: | ---: |
| E0107 generic argument count | 136 | 0 | 0 | **-136** |
| E0308 mismatched types | 134 | 152 | 145 | +11 |
| E0609 no field | 17 | 17 | 17 | 0 |
| E0282 type annotations needed | 10 | 0 | 0 | **-10** |
| E0121 type placeholder | 10 | 0 | 0 | **-10** |
| E0382 use of moved value | 8 | 8 | 8 | 0 |
| E0277 trait bound | 5 | 30 | 27 | +22 |
| E0063 missing field | 5 | 0 | 0 | **-5** |
| E0631 closure signature | 0 | 5 | 5 | +5 |
| E0425 unresolved name | 2 | 2 | 2 | 0 |
| E0599 no method | 1 | 1 | 1 | 0 |
| E0271 associated type | 1 | 1 | 1 | 0 |
| **total** | **329** | **217** | **206** | **-123** |

The baseline column is this worktree's own reproduction of the round-30 brief's
319. It reads 329 because it counts the 10 `E0282`s the brief's table does not
list; on the brief's accounting the crate goes **319 -> 196**.

The `E0308`/`E0277`/`E0631` rises are not regressions in the ordinary sense:
they are the next layer, now reachable. A reference that used to be REJECTED for
its arity now type-checks its arity and fails on its arguments instead. Nothing
was erased to produce them.

## Item 1 — type-parameter defaults (`E0107`, 136)

**The rule.** A type reference may omit TRAILING type arguments whenever every
omitted parameter declares a default: `class Slot<T, U = string, V = number>`
referenced as `Slot<boolean>` MEANS `Slot<boolean, string, number>`, and a
declaration whose parameters are all defaulted may be referenced with no
argument list at all. A default is lowered in the declaration's own parameter
scope, so a later default may mention an earlier parameter (`<A, B = A[]>`) and
the substitution map is built left to right.

Interfaces and type aliases already took their defaults, through
`type_argument_substitution`. A CLASS reference fell through the table in
`type_reference_to_hir` to a fallback that interned `Type::Class` with whatever
short list the source wrote.

**Where it lands.** `ModuleBuilder::type_arguments_with_defaults`
(`lowering/ty/generics.rs`) completes a short list, or returns `None` when some
omitted parameter has no default — which `tsc` rejects except where inference
supplies it, so the caller keeps its short list rather than inventing an
argument. Both the type-reference fallback and `class_extends_clause` consult
it.

**The dynamic boundary, stated at the lowering site.** A parameter defaulted to
`any` (`E extends Env = any`) lowers to `Type::Unknown`. Source `any` in a type
position IS a dynamic boundary under CLAUDE.md — the declaration itself says
"whatever the instantiation supplies, unchecked" — and taking the default is
strictly better than dropping the argument, because a dropped argument leaves an
arity no later stage can repair. No `SmeltUnknown` was introduced to make
anything compile.

## Item 2 — the same rule, crate-wide and correctly keyed

Item 1 alone moved 136 `E0107`s by ONE. Two defects, both found by measuring
rather than by reading:

1. **The registry has to outlive the module.** It was on `TypeScope`, covering
   only the classes of the module being lowered. A dependency cycle through a
   barrel file routinely lowers a CONSUMER first — hono's `types.ts` and
   `context.ts` are exactly that pair — and neither a per-module registry nor
   `find_class` (no `Item::Class` yet) can answer for it. The map moved to
   `HirCtx`, is filled by the crate-wide predeclaration pass
   (`predeclare_type_declarations_with_path`) before ANY body is lowered, and is
   carried across manifest entries by the transpiler's lowering state like the
   other cross-entry type surfaces. The per-module call stays for standalone
   lowering (`dump-hir`, the frontend's own tests).
2. **The key has to be the declaration's own symbol.** It was the bare interned
   spelling. Where a crate declares one spelling twice, the ambiguous name is
   renamed per module (`HirCtx::type_renames`), so the bare spelling merges two
   unrelated declarations. Hono declares a generic
   `class Context<E extends Env = any, P extends string = any, I extends Input = {}>`
   and, in `src/middleware/cache/index.test.ts`, a plain
   `class Context implements ExecutionContext` — which is why the real one is
   emitted as `Context_1`. A reference to the renamed generic one resolved to a
   symbol the registry had no entry for, and a reference to the plain one must
   not pick up the generic one's defaults either.

**The brief's items 1 and 2 are one family, not two.** The 10 `E0121`s
(`HonoRequest<_, _>`, `Context_1<_, _, _>` written with `_` in item signatures)
and the 5 `E0063`s (`_smelt_phantom` missing in initializers) are the arity
gap's other face: a reference of the wrong arity is what produced the
placeholders, and they close with it rather than needing a rule of their own.
The 10 `E0282`s went with them. No separate change was needed or made.

## Item 3 — tuple recovery at the erased-callable boundary

Two of the three parts H42 left landed; the third turned out to be a different
family, and is recorded below rather than papered over.

**3a. `SmeltFromUnknown for (A, B)`.** `IntoSmeltUnknown for (A, B)` had existed
since tuples were first erased; the inverse had not. So the H42 recovery arm —
`<T as SmeltFromUnknown>::smelt_from_unknown(..)`, taken exactly where the
render position CAN spell `T` — could not compile the moment `T` was
instantiated at a tuple, which is what Hono's `Router<[unknown, RouterRoute]>`
is. A missing impl on one side of a round trip is not a reason to erase the
value on the other, so the fix is the impl. Arity mirrors `IntoSmeltUnknown`
exactly (2); adding an arity to one side and not the other would reintroduce the
asymmetry this closes. **-3 `E0277`, -1 `E0599`.**

**3b. An erased argument at an unspellable type parameter.**
`function_args_from_smelt_args_text` rendered each argument with
`extract_value_text` at the AMBIENT render scope. For a parameter whose declared
type is a `Type::TypeParam` the position cannot spell, that arm answers
`SmeltUnknown` — a claim about the callee's Rust signature that the position is
in no state to make, because the instantiation was fixed where the value was
created. A method of `Router<[unknown, RouterRoute]>` erased into a callable
really takes `&(SmeltUnknown, RouterRoute)`.

An ARGUMENT position is exactly where the right answer needs no scope: the
callee's signature IS the expected type. `erased_argument_at_param_text` renders
`SmeltFromUnknown::smelt_from_unknown(..)` with an INFERRED target and lets
rustc solve it from the position; the identity impl on `SmeltUnknown` makes a
genuinely erased parameter render unchanged. This is the H42 rule — the render
position decides what a type parameter means — at the one position whose
decision is not the emitter's to make. It is deliberately narrow: only a bare
type parameter with no Rust name in scope takes it. **-7 `E0308`
(`(SmeltUnknown, RouterRoute)`), 8 of the family gone, 1 left.**

## FOUND AND NOT FIXED — a substituted type parameter loses its declaration's ABI

The one remaining `(SmeltUnknown, RouterRoute)` error, and the reason item 3 has
no end-to-end corpus fixture. This is a fourth family, not a residue of item 3.

`param_type_is_by_shared_reference` answers **true** for `Type::TypeParam`, so a
callable whose declared parameter is `T` is emitted as `Fn(.., &T)` — both in a
class's function-valued field and in a generic interface's method slot. It
answers **false** for a tuple, which is correct on its own terms: a non-generic
`(label: string, value: [string, number]) => void` really is emitted as
`Fn(String, (String, f64))`, by value, and is called that way.

The two answers collide when a GENERIC callable is reached through a value whose
type arguments are already fixed. MIR hands the call site the SUBSTITUTED
parameter type, so the predicate sees the tuple and packs the argument by value,
while the callee's Rust type is still `&T` — i.e. `&(String, f64)`.

Minimal reproduction, which compiles under `tsc` and runs under Node:

```ts
interface Store<T> {
  add(label: string, value: T): void;
  size(): number;
}
class ListStore<T> implements Store<T> {
  entries: T[] = [];
  add(label: string, value: T): void { this.entries.push(value); }
  size(): number { return this.entries.length; }
}
function fill(store: Store<[string, number]>): number {
  store.add("first", ["x", 1]);
  return store.size();
}
console.log(fill(new ListStore<[string, number]>()));
```

```
struct Store<T> { add: ::std::rc::Rc<dyn Fn(String, &T) -> ()>, .. }
...
(store.add.clone())("first".to_owned(), { /* (String, f64) by value */ })
                                          ^ expected `&(String, f64)`
```

The rule the fix needs: **the ABI is the DECLARATION's, and substituting a type
parameter at a call site must not change it.** Today the substituted
`FunctionType` carries no record of which parameters were type parameters, so
the call site cannot tell `T = (string, number)` (by reference, because the slot
is `&T`) from a parameter declared `[string, number]` (by value). Either MIR
keeps that provenance on the instantiated `FunctionType`, or the interface /
field slot is emitted by value for a type parameter too — the second is the
simpler statement but reverses a decision `param_type_is_by_shared_reference`
documents at length, so it needs its own measurement.

It is not tuple-specific: any concrete instantiation whose Rust type is passed
by value hits it. It predates this round — nothing in items 1-3 changed the
packing decision.

A corpus fixture for it (`91_erased_callable_tuple_parameter`) was written,
verified against Node, and REMOVED, because it fails on this seam rather than on
anything item 3 fixes. The tuple recovery it was meant to pin is instead pinned
by `a_type_parameter_instantiated_at_a_tuple_round_trips_through_the_carrier` in
`crates/smelt-codegen-rust/tests/type_param_render_scope_runtime.rs`, the H42
tier — the same placement, and for the same reason, H42 chose: every shape that
exercises the recovery needs a value that has already crossed into `unknown`,
which the line-based classifier counts as avoidable erasure, and the examples
corpus is a hard `avoidable == 0` invariant.

## ALSO FOUND AND NOT FIXED — a subclass of a GENERIC base erases its inherited members

`effective_class_methods` / `effective_class_fields` flatten a subclass by
re-emitting the base's members verbatim, with no substitution of `base_args`.
For a monomorphic subclass of a generic base that is wrong twice over:

```
class Slot<T, U = string, V = number> { .. label(): U { return this.second; } }
class TaggedSlot extends Slot<boolean> { .. }
```

```
struct TaggedSlot { first: SmeltUnknown, second: SmeltUnknown, .. }
impl TaggedSlot { fn label(&self) -> U { .. } }   // E0425: no `U` in scope
```

The inherited FIELDS erase to `SmeltUnknown` and the inherited METHODS keep the
base's own parameter names in their signatures. Item 1's heritage-clause rule is
a precondition for fixing this — without the full `base_args` there is nothing
to substitute WITH — but the substitution itself is missing, and it is the
round-29 "generic-base null field" outcome's neighbourhood, which the round-30
brief explicitly defers. So the `extends` case is asserted in
`heritage_clause_takes_base_type_parameter_defaults` (it checks `base_args`
directly) rather than in fixture 90.

## Tests added

| test | what it pins |
| --- | --- |
| `class_reference_takes_trailing_type_parameter_defaults` | `Slot<boolean>` against `<T, U = string, V = number>` is 3 arguments |
| `class_reference_with_no_arguments_takes_every_default` | a fully defaulted declaration referenced bare |
| `later_type_parameter_default_sees_earlier_arguments` | `Pair<boolean>` against `<A, B = A[]>` is `Pair<boolean, boolean[]>` |
| `class_reference_forward_to_a_later_declaration_takes_defaults` | class TYPES hoist within a file |
| `class_reference_without_defaults_invents_no_arguments` | the rule is defaults, not padding |
| `heritage_clause_takes_base_type_parameter_defaults` | `extends Slot<boolean>` records 3 base arguments |
| `class_reference_takes_defaults_from_a_module_lowered_later` | the crate-wide half: consumer lowered first |
| `a_type_parameter_instantiated_at_a_tuple_round_trips_through_the_carrier` | `SmeltFromUnknown for (A, B)`, asserted on the VALUE |
| fixture `90_type_parameter_defaults` | field / parameter / return / bare reference, against Node 22 |

## SmeltUnknown

No conversion was introduced to make generated Rust compile. Item 1 REMOVES
erasure (a defaulted argument is the type the source means, in place of a
dropped one); item 3b removes it too (a recovery in place of an assertion). The
one `Type::Unknown` this round adds is an `any` DEFAULT, which is a source
boundary, documented at the lowering site in `type_arguments_with_defaults`.

Examples invariant: avoidable **0**, delta **+0**. Runtime prelude grew by the
one new blanket impl (+196 occurrences across 87 files) and the baseline is
re-snapshot for that growth alone.
