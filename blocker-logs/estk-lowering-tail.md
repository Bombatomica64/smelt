# es-toolkit lowering tail blockers

Branch: `claude/estk-lowering-tail`. Source: `toss/es-toolkit` @ `e008a2818cd8`.

Seven small singleton es-toolkit lowering blocker families were investigated.
Six are fixed through general rules (no per-source special cases); one is
deliberately deferred with a reason. Each sub-task lists the exact source usage
found (byte-span diagnosed via the fixed binary's `dump-hir`), the general rule
implemented, and how it was verified.

Verification tooling: the fixed binary's `dump-hir` (per-file, run against the
pinned checkout with the tracked fixture `Smelt.toml`), focused end-to-end
fixtures (`smelt build` + generated `cargo test`, 13 tests green), in-repo
regression tests (`crates/smelt-frontend-ts/src/tests/{class_module_tests,
part04_tests,part05_tests}.rs`), `cargo test --workspace --exclude smelt-gui`
(all green), lib `cargo clippy` (pedantic, clean) on the two touched crates, and
the whole-crate `smelt build` abort point before/after (unchanged — see below).

## 1. Default `sort()` over a leaked type-parameter list

- Files: `src/compat/object/values.spec.ts`, `src/compat/object/valuesIn.spec.ts`.
- Diagnostic (was): `array sort supports boolean, number, and string arrays for
  now` on `values(value).sort()` inside
  `map(vals, value => values(value).sort())`.
- Root cause: a recent merge generalized comparator-less `sort()` for
  `unknown`/union element lists, but the cross-module generic `values<T extends
  object>(...): Array<T[keyof T]>` call leaks an unsubstituted **type parameter**
  as the element type (`List<T>`), which that fix did not cover.
- Rule: a `Type::TypeParam` element joins the erased/union surfaces accepted for
  the default `ToString`-coercion sort, mirroring how the sort *comparator*
  return check and `erased_or_union_surface` already treat `TypeParam` as erased
  (`stdlib.rs`). In the emitter (`list_mutation.rs`), a non-scoped `TypeParam`
  element — which renders as `SmeltUnknown` — takes the same string-coercion
  path as `unknown` (factored into `default_sort_by_string_coercion_text`); a
  type parameter that IS in scope renders as a real generic and is intentionally
  left rejected at codegen (it has no `into_smelt_unknown`).
- Verified: both files now lower clean (`dump-hir` exit 0); e2e fixture
  (`gen_sort.spec.ts`, cross-module generic `myValues(obj)` producing `List<T>`)
  sorts and passes; regression test
  `lowers_default_sort_over_type_parameter_list`.
- Residual (separate, pre-existing, orthogonal): sorting a **call-result
  temporary** (`values(value).sort()` where the receiver is a fresh temp local)
  emits `_smelt_tmp.sort()` on a non-`mut` local (E0596). This is independent of
  the element type — a concrete `number[]` temp fails identically — and is not
  part of this family.

## 2. `toContain` with an optional needle

- File: `src/compat/array/sample.spec.ts`.
- Diagnostic (was): `expect(...).toContain(...) requires a string, array, set,
  or tuple actual value with a matching expected value` — the dispatched first
  error was `expect(values).toContain(actual)` where `values =
  Object.values(object)` (a `List`) and `actual = sample(object)` is `T |
  undefined` (an `Optional`).
- Rule: `sample(...)`-style helpers return `T | undefined`, so the needle is
  commonly `Optional(T)` while the collection holds `T`. JS containment compares
  the needle against each element regardless of nullability, so `contains_expr`
  (`matchers.rs`) accepts an optional expected whose inner type matches the
  element type, or whose inner type is erased (`unknown`/leaked type param). The
  list/set/tuple emitters (`list.rs`, `set.rs`, `tuple.rs`) unwrap the optional
  and guard on `Some` (a `None`/`undefined` needle is never contained); when the
  inner type is erased, each concrete element is erased to `SmeltUnknown`
  (`erase_value_text`) and compared with `same_js_key`, matching JS semantics.
- Verified: `sample.spec.ts` now lowers clean (`dump-hir` exit 0); e2e fixtures
  (`tocontain.spec.ts`, `tocontain_erased.spec.ts`) cover list/tuple/set actuals
  with concrete-inner and erased-inner optional needles, all green; regression
  test `expect_to_contain_accepts_optional_expected_in_collection`.

## 3. `this` type in a class declared inside a function body

- File: `src/compat/function/memoize.spec.ts`.
- Diagnostic (was): `this class type is not resolvable yet` on `return new
  ImmutableCache() as this;` inside `override clear(): this` of a class declared
  inside the `describe` callback.
- Root cause: a class declared inside a function body is lowered inline without
  the forward-declaration pass that registers top-level classes, so it is not in
  the class table while its own method bodies (and their `: this` annotations)
  are lowered.
- Rule: the `TSThisType` handler (`ty/annotations.rs`) now falls back to
  interning the enclosing class name directly (`intern_type_name(current_class)`)
  when the fully-lowered class item is not yet registered. The interned symbol is
  identical to the class item's name (both come from `intern_type_name(class_
  text)`), so the resolved `Type::Class` is the same one the registered path
  would produce.
- Verified: the `this class type is not resolvable yet` diagnostic is gone;
  e2e fixture (`nested_this.spec.ts`, class in a `describe` body with `self():
  this` and `rebuild(): this` returning `new Node(...) as this`) lowers,
  compiles, and passes; regression test
  `lowers_this_type_in_class_declared_in_function_body`.
- Residual (out of scope): `memoize.spec.ts` now aborts later at `instanceof
  Array` (`TypeScript instanceof target \`Array\` is not a lowered class`), a
  different family. Note also that a `this`-typed method *return value* is erased
  to `SmeltUnknown` by the emitter for all classes (top-level too) — a
  pre-existing emitter behavior unrelated to this frontend fix.

## 4. Static field with a function/arrow initializer

- File: `src/compat/object/cloneDeep.spec.ts`.
- Diagnostic (was): `static fields require a concrete literal initializer` on
  `static c = function () {};` in `class Foo { ... }`.
- Rule (general, incremental): a static field whose initializer is a function or
  arrow expression is a static *callable* member, not a data constant.
  Materializing it as an associated function needs the static-method lowering
  path, which is not yet wired to property initializers; until then such a field
  is skipped rather than blocking the whole class (`decls/functions.rs`). This
  keeps classes that merely carry the member lowerable (its value is a function,
  never read back as structured data); an actual `Class.c(...)` use would fail at
  the call site rather than silently reading a wrong value.
- Verified: `cloneDeep.spec.ts` now lowers clean (`dump-hir` exit 0); e2e fixture
  (`static_fn.spec.ts`, `class Widget { a; b; static make = function(){};
  static build = () => 42; }`) lowers, compiles, and constructs; regression test
  `lowers_class_carrying_static_function_field` (instance fields survive, the
  function-valued statics are not materialized as data static fields).

## 5. `class extends Object`

- File: `src/predicate/isPlainObject.spec.ts`.
- Diagnostic (was): `base class \`Object\` is not declared` on `new (class
  extends Object {})()`.
- Rule: `class X extends Object {}` names the universal root constructor as its
  base. Every JavaScript class already descends from `Object`, so an explicit
  `extends Object` contributes no fields, methods, or distinct constructor
  behavior: `class_extends_clause` (`decls/functions.rs`) lowers it as no
  declared base (the subclass keeps its own constructor identity, so its
  instances are still non-plain objects). A user-declared class or value import
  literally named `Object` still shadows the global via the normal declared-base
  path.
- Verified: the `base class \`Object\`` diagnostic is gone; e2e fixture
  (`extends_object.spec.ts`) lowers, compiles, and constructs with correct field
  reads; regression test `lowers_class_extending_object_as_empty_base` (base is
  `None`).
- Residual (out of scope): `isPlainObject.spec.ts` now aborts later at
  `unresolved class \`Request\`` (host global), matching the existing
  `Smelt.toml` note.

## 6. `new Set()` assigned to an optional-typed binding

- File: `src/compat/predicate/isMatchWith.spec.ts`.
- Diagnostic (was): `new Set() requires a Set<T> type annotation` on `set1 = new
  Set()` where `let set1: Set<unknown> | undefined`.
- Root cause: the empty `new Set()` path errored when its contextual type hint
  was not directly a `Set`, whereas the empty `new Map()` path already fell back
  gracefully to an empty dict.
- Rule: the empty `new Set()` path (`expr/operators.rs`) now recovers the set
  element type from the contextual hint even when the `Set<T>` is wrapped in
  `Optional`/`Union` (new `set_type_from_hint` helper unwraps those arms), and
  otherwise falls back to an empty `Set<unknown>` (mirroring `new Map()`) instead
  of rejecting the construction — the element type is refined by later `.add(...)`
  usage.
- Verified: `isMatchWith.spec.ts` now lowers clean (`dump-hir` exit 0); e2e
  fixture (`set_hint.spec.ts`) constructs empty and populated sets from an
  optional `Set<number>` annotation and queries them; regression test
  `lowers_new_set_assigned_to_optional_set_binding` (empty `new Set()` lowers to
  `SetLit`).

## 7. Unannotated parameter of a nested constructor-function — DEFERRED

- File: `src/compat/predicate/matchesProperty.spec.ts`.
- Diagnostic: `function parameters must have explicit type annotations or default
  initializers` on `function Foo(object) { Object.assign(this, object); }` (a
  nested constructor-function used via `new Foo({ ... })`).
- Why deferred: the hint anticipated an inferable callback position, but the
  failing parameter is on a plain (non-callback) nested `function` declaration
  used as a constructor. There is no contextual type to infer it from, and TS
  itself types it `any` (the source `@ts-ignore`s it). Making all unannotated
  parameters default to `SmeltUnknown` would be a sweeping, unsound policy change
  contrary to the repo's SmeltUnknown enforcement; call-site-directed parameter
  inference for arbitrary constructor-functions is a genuine feature, not a tail
  blocker. Left with the honest diagnostic pending that work.

## Whole-crate `smelt build` abort point

Before and after these changes, `smelt build` at the pinned es-toolkit checkout
aborts at the same file — `src/predicate/isEqualWith.spec.ts` (`dynamic computed
access on the global object requires the runtime global object`), which belongs
to a sibling agent. All six fixed files sort after that abort, so they do not
move the abort point earlier; the fixes are exercised via `dump-hir` and the
focused fixtures instead.
