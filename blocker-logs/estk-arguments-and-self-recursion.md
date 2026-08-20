# es-toolkit runtime pass: nine general defects, 875 -> 908 passing

Measured against es-toolkit at the ref pinned in `.github/compat/libraries.json`
(`e008a281`) with the fixture manifest `.github/compat/es-toolkit/Smelt.toml`,
starting from Smelt `4d15304` (`main`, the merge of #192).

## Result

| Corpus | Before | After |
| --- | --- | --- |
| es-toolkit | 875 passed / 184 failed | **908 passed / 151 failed** |
| es-toolkit probe blockers | 0 | 0 |
| remeda | 1789 passed / 0 failed | **1789 passed / 0 failed** |

Nine independent defects, found by reading the largest failing groups rather
than the individual specs. Each was a *silent wrong answer* or a panic, not a
compile error: the generated crate builds at zero errors throughout.

## Defect 1 — an `arguments` object was not iterable

`Array.from(arguments)` and `[...arguments]` both **panicked** with
`unknown is not iterable`. Thirteen specs died in that panic before reaching any
assertion, all of them declaring the same helper shape:

```ts
function fn(_a: unknown, _b: unknown, _c: unknown) {
  return Array.from(arguments);
}
```

`rest` (4), `ary` (3), `flow`/`flowRight` (2), `partial`/`partialRight` (2),
`memoize` (1), `unary` (1).

**Root cause.** Smelt models `arguments` as an array-like marker record —
`{ __smelt_arguments: true, "0": …, "1": …, length: n }` — built by
`smelt_arguments_object`. The three erased iterable-to-list coercion templates
(`coercion.rs`, list-of-unknown / list-of-string / list-of-T) walk an erased
object through: byte-buffer elements, then `__smelt_map`, then `__smelt_set`,
then `__smelt_symbol_iterator`, then `panic!`. The marker record carries no
`__smelt_symbol_iterator` slot, so every `arguments` object hit the panic.

**Fix.** A new `smelt_arguments_elements` runtime door emitted next to the
constructor it mirrors, consulted by all three templates. It reads `length` and
the index keys, not the record's raw key order, so a named property assigned onto
the record cannot perturb the element sequence.

**Why keyed on the marker and not on "has a `length`".** `Array.from` accepts any
array-like; a bare array-like is *not* iterable. One emitter serves both
spellings here, so widening the arm to any `length`-bearing record would make
`[...{ length: 0 }]` succeed where JavaScript throws. An `arguments` object is
accepted because it genuinely *is* iterable (its `Symbol.iterator` is
`Array.prototype.values`), not because it is array-like.

**Measured:** 875 → 876, 0 newly failing. Only one of the thirteen flipped to
passing; the other twelve now reach their assertions and fail on defect 3 below.

## Defect 2 — a named function expression could not call itself

A named function expression binds its own name inside its own body, and that is
how JavaScript writes a self-recursive callback. es-toolkit `toMerged` is built
entirely out of that shape:

```ts
return mergeWith(cloneDeep(target), source, function mergeRecursively(targetValue, sourceValue) {
  if (Array.isArray(sourceValue)) {
    …
    return mergeWith(clone(targetValue), sourceValue, mergeRecursively);
  }
  …
});
```

**Root cause.** `function_expression_value` never bound `function.id`. The
self-reference therefore reached the end of `identifier_expression`, matched the
`source_contains_forward_callable` fallback, and lowered to a **module global
that resolves to an empty record**:

```rust
_smelt_tmp_7 = SmeltRecord::from([]);
_smelt_tmp_8 = SmeltUnknown::Object(SmeltObject::from_unknown_record(_smelt_tmp_7));
let _smelt_tmp_9 = merge_with_975(…, …, /* customizer: */ _smelt_tmp_8);
```

Calling an empty object collapses to a null callback rather than failing, so the
recursion **silently did nothing**: every nested level fell back to `mergeWith`'s
default behaviour and the caller saw a partially merged result. A silent wrong
answer, not a crash.

An inline Rust closure cannot express the binding either — the closure would have
to capture the very binding it is being assigned to, which the borrow checker
rejects.

**Fix.** Lift a self-recursive named function expression to a module-owned
function item, which is what a hand port would write: the recursion becomes
ordinary `fn` recursion and the value handed to the caller is the same
`item_function_closure_expression` wrapper a named top-level function reference
already produces.

Three parts:

1. **`function_expression_item_into_slot`** — the existing lift, plus an
   already-reserved `ItemId` to write into. The reservation is made *before* the
   body is lowered so the body can already reference the item; it is the same
   mechanism `function_declaration` uses for hoisted local declarations
   (`LocalScope::function_item`). The reservation carries the signature the real
   lowering will derive (annotation, else the contextual callable hint, else
   `unknown`), because a self-reference lowered against it reads its parameter
   types.
2. **Scoping** — the source name is inserted into `items` for exactly the
   duration of the body lowering and the previous entry restored afterwards, so
   the name never becomes visible to the rest of the module. That is precisely
   JavaScript's rule for a named function expression, and it is the pattern
   `specialization.rs` already uses for capture names.
3. **Detection** — `collect_statement_capture_names` reports only names that are
   *already bound in the enclosing scope*: it is a capture collector, not a
   free-variable collector, and a named function expression's own name is by
   definition not bound out there. Rather than add a second visitor that would
   drift from it, the self-recursion probe binds the name to a placeholder local
   for the duration of one scan. The same collector, used unmodified, answers the
   capture question.

**Preconditions.** The lift applies when the body references its own name and
captures nothing from the enclosing body — a module item has no access to the
surrounding body's locals. A capturing self-recursive function expression keeps
the closure path unchanged (see "Known gaps").

**Measured:** 876 → 883, 0 newly failing. Seven of the eight `toMerged` specs.
Name collisions are handled by the emitter's existing suffixing (`step_2` /
`step_4` for two same-named lifts in one crate), covered by a runtime test.

## Defect 3 — containment against a narrowed union receiver folded to `false`

`trim`, `trimStart` and `trimEnd` all accept `chars?: string | string[]` and
dispatch on it:

```ts
switch (typeof chars) {
  case 'string': { … while (str[startIndex] === chars) startIndex++; … }
  case 'object': { while (startIndex < str.length && chars.includes(str[startIndex])) startIndex++; }
}
```

Every array-`chars` spec answered the **untrimmed string**. Ten of them: `trim`
(3), `trimStart` (4), `trimEnd` (3). One — "should return the string unchanged
when none of the leading characters in the array match" — passed *vacuously*,
which is the tell.

**Root cause, and where it is not.** The frontend is right: `chars?:` interns as
`Optional(Union([String, List(String)]))`, the `typeof` switch narrows it, and
`list_contains_call` emits `Rvalue::ListContains` with a `List<String>` receiver.
MIR is right too — `%14 = list_contains copy %1, copy %0[copy %2]`. But MIR reads
the value through its **declaring local**, whose type is still
`Optional<String | List<String>>`, so the *operand* type at emission is the wide
one. `list_contains_text` matched `Type::List` alone:

```rust
let Some(Type::List(list_item_ty)) = self.mir.types.get(list_ty) else {
    return Ok("false".to_owned());
};
```

so the whole call became a literal `false`, the loop never advanced, and
`substring(0)` returned the input.

**Fix.** A `list_receiver_surface` helper projects the operand to its single list
arm: unwrap an `Optional`, then select the one `List` member through the existing
`project_union_value_text`. The projection is safe precisely because the frontend
established the narrowing before emitting the rvalue, and
`project_union_value_text` emits an `unreachable!` guard on the other arms — so a
receiver that somehow held a different member aborts rather than answering a wrong
`false`. A union with more than one list member stays ambiguous and still yields
`None`.

Note what was *not* changed: `list_surface_type` in the frontend already handles
this shape, and widening it was tried and reverted as unnecessary. The narrowing
never failed; only the emitter's operand-type test did.

**Measured:** 883 → 893, 0 newly failing — the whole trim family.

## Defect 4 — `Math.round` used Rust's tie rule

JavaScript rounds a tie toward **+∞**; Rust's `f64::round` rounds a tie away from
zero. `Math.round(-1.5)` is `-1` in JavaScript and `-2.0` in Rust. es-toolkit's
`round` specs assert the JavaScript answer and say so in a source comment,
because it surprises people.

Three specs: `rounds a number to zero decimal places by default`, `handles
negative numbers properly`, `rounds correctly with edge cases`.

**Fix.** A `smelt_math_round` prelude helper, gated on the op actually appearing
(`needs_math_round`), so a crate that never rounds emits byte-identical output.
`floor`/`ceil`/`trunc` mean the same thing in both languages and keep mapping
straight to their `f64` methods; the emitter's `method_name` match makes that
split explicit by returning `None` only for `Round`.

Computed as `floor(x)` plus one when the fraction reaches `0.5`, **not** as
`floor(x + 0.5)`: the ECMA-262 note on `Math.round` calls out that the naive form
is wrong for very large `x`, where adding `0.5` is not representable. `floor` is
exact at those magnitudes and the fraction is then `0`, so the value passes
through unchanged. `-0` is preserved, because `Math.round(-0.5)` is `-0` in
JavaScript and `Object.is(-0, 0)` is `false`.

Verified against Node directly:
`Math.round(1.5) 1.4 -1.5 -1.4 -2.5 0.5 -0.5` → `2 1 -1 -1 -2 1 -0` from both.

**Measured:** part of the 893 → 900 step below.

## Defect 5 — a call to an assertion *overload* was dropped entirely

es-toolkit `invariant` is declared as two assertion overloads plus an
implementation:

```ts
export function invariant(condition: unknown, message: string): asserts condition;
export function invariant(condition: unknown, error: Error): asserts condition;
export function invariant(condition: unknown, message: string | Error): asserts condition { … }
```

All four of its specs failed, and the generated arrow body shows why — the call is
simply **not there**:

```rust
_smelt_tmp_5 = ::std::rc::Rc::new(|| -> Result<(), Box<dyn std::error::Error>> {
    let _smelt_tmp_0: bool = false;
    _smelt_tmp_0.clone();
    Ok::<(), Box<dyn std::error::Error>>(())
});
```

`expect(() => invariant(false, 'This should throw')).toThrow(…)` evaluated the
argument and nothing else.

**Root cause.** `function_declaration` maps an assertion return (`asserts x`) to
`Type::None`, because that is what such a function returns at runtime.
`overload_signature` did not: it lowered the annotation structurally, and a
`TSTypePredicate` is boolean-shaped, so the overload's `return_ty` came out
`Bool`. The selected overload's return type is what types the call's *destination*
— MIR shows `%0 = call fn1(false, "nope")` with `%0: Bool` against a `-> None`
function — and codegen, unable to reconcile them, dropped the call and left the
argument.

Isolated by bisecting the shape: single-signature + `asserts` works, overloads +
`void` works, overloads + `asserts` drops the call.

**Fix.** `overload_signature` now checks `assertion_return_type` first and interns
`Type::None` for an assertion overload, exactly as the declaration path does. A
`value is T` *predicate* overload deliberately keeps `Bool`: a type predicate
really does return a boolean and its callers read it.

**Measured:** 893 → 900 together with defect 4 (`round` 3, `invariant` 4), 0 newly
failing.

## Defect 6 — `arguments` saw the declared arity, not the actual call

This is the gap defects 1-5 left open, and the largest of them.

A JavaScript `arguments` object is the **actual** argument list of the call, not
the function's declared parameter list, and the two differ constantly:

```js
function fn(_a, _b, _c) { return Array.from(arguments); }
ary(fn, 2)('a', 'b', 'c', 'd');   // fn('a', 'b')      -> ['a', 'b']
rest(fn)(1, 2, 3, 4);             // fn(1, 2, [3, 4])  -> [1, 2, [3, 4]]
```

Smelt emitted such a function at its **declared** arity, and the erased-call
boundary padded a short call up to it:

```rust
Rc::new(move |smelt_args: Vec<SmeltUnknown>| callback(
    smelt_args.get(0).cloned().unwrap_or(SmeltUnknown::Null),
    smelt_args.get(1).cloned().unwrap_or(SmeltUnknown::Null),
    smelt_args.get(2).cloned().unwrap_or(SmeltUnknown::Null),
))
```

By the time the body ran the count was gone, so `arguments` answered
`['a', 'b', null]`. The padding cannot simply be *trimmed* either: a real
trailing `undefined` is a legitimate argument — es-toolkit's `partial`
placeholder specs expect `[undefined, 'b', undefined]` — so the count has to
travel with the call.

**Fix: a function that reads `arguments` is lowered variadically.** Its parameter
list becomes a single rest parameter holding the whole argument list, and each
declared name is re-bound from that list at function entry:

```ts
// source                          lowered as
function fn(a, b, c) {             function fn(...__smelt_arguments) {
  … arguments …                      const a = __smelt_arguments[0];
}                                    const b = __smelt_arguments[1];
                                     const c = __smelt_arguments[2];
                                     … __smelt_arguments …
                                   }
```

This reuses the rest-parameter path end to end rather than inventing a side
channel: callers already pack their arguments into the rest list, the erased-call
boundary already forwards its whole `Vec<SmeltUnknown>` into it, and `arguments`
is then that list — exact count, no padding. The new
`lowering::arguments_forwarding` module carries the rewrite and its reasoning;
detection is an oxc `Visit` scanner that descends into arrow functions (which have
no `arguments` of their own) and stops at every non-arrow `function` boundary
(which has). It is wired into all three non-arrow function-lowering sites: module
function declarations, nested function declarations, and function expressions.

**A thread-local "current call arguments" slot was the alternative, and was
rejected.** Nothing identifies which callee a pushed frame belongs to, so a
function reading `arguments` through a *direct* call nested inside an erased call
would read the outer frame. Closing that needs callee identity at the erasure
site, which is exactly the information the rest list already carries.

Functions that never mention `arguments` are untouched and keep their declared
arity, so the change costs nothing for the overwhelming majority of code. remeda
contains **zero** `arguments` references, which is why it cannot be affected at
all.

**Measured:** 900 → 904, 0 newly failing (`ary` 3, `rest` 1).

## Defect 7 — `Function.prototype.length` did not survive erasure

`length` is the **declared** arity — `fn(a, b, c).length` is `3` — and real code
branches on it. es-toolkit `rest(func)` defaults its split point to
`func.length - 1`, and `ary(func)` to `func.length`.

Two problems, one exposed by the other:

* A typed callable knows its arity and `SmeltErasedFunction` carries it in a
  `length` field, but erasing to `SmeltUnknown::Function(Rc<…>)` throws the field
  away — an `Rc<dyn Fn>` has nowhere to put it — so the erased `.length` read
  answered `0`, making `rest`'s default `-1`.
* Defect 6 removes the declared arity from the signature, so the nested-function
  site had to record it explicitly rather than derive it.

**Fix.** A `SMELT_FUNCTION_LENGTHS` registry keyed by the callable's *canonical*
identity — the same key `smelt_same_function_identity` compares by, so a chain of
erasure wrappers resolves to the arity of the function the chain started from.
All three paths that mint an erased callable now record it:
`SmeltErasedFunction::into_smelt_unknown`, the function-item value accessor, and
the closure-local erasure adapter. The erased `.length` read consults it (and a
callable object reports the arity of the callable in its `__smelt_call` slot).
The nested-function site records the source arity in `required_params`, which is
the field `length` already derives from and which does still describe the source
contract.

**Measured:** 904 → 907, 0 newly failing (the remaining three `rest` specs).

## Defect 8 — an erased-ABI call had its return type applied twice

Found while validating defect 6, and a latent bug it makes reachable rather than
one it introduced.

`is_erased_unknown_rest_function` keys **only on the parameter shape** — one rest
parameter of `unknown[]` — so any rest-only function is represented as
`SmeltErasedFunction`, whose `call` answers `SmeltUnknown` whatever the declared
return type is. Three callback adapters called through that ABI and then applied
the *declared* return type to the result anyway, emitting

```rust
SmeltUnknown::Array(smelt_callback.call(…).into())   // E0277: SmeltArray: From<SmeltUnknown>
```

Unreachable before, because a rest-only function with a concrete return type was
rare; the variadic rewrite makes every `arguments`-reading function rest-only, so
`function f(...): unknown[]` — the natural spelling of a spec helper — hits it and
the generated crate does not compile.

**Fix.** All three adapters (`rest_vector_unknown_adapter_text`, the typed-rest
adapter, and `erased_rest_forwarding_closure_text`) now treat the call's value as
already erased when it went through the erased ABI. Where the declared return was
already `unknown` — the overwhelmingly common case — the emitted text is
unchanged.

## Defect 9 — a static property on a function declaration was dropped

JavaScript functions are objects, so a module can hang a value off one. es-toolkit
publishes all its placeholder sentinels that way, nine sites in all:

```ts
export function partial(func, ...partialArgs) {
  return partialImpl(func, partial.placeholder, ...partialArgs);
}
partial.placeholder = Symbol('compat.partial.placeholder');
```

Also `partialRight.placeholder`, `curry.placeholder`, `curryRight.placeholder`,
`bind.placeholder`, `bindKey.placeholder` and `memoize.Cache = Map`.

**Root cause.** The assignment lowered into the module-init function — which
nothing ever calls — and the *target* was dropped outright, leaving the generated
init body binding the right-hand side to a local and discarding it:

```rust
pub(crate) fn curry_1966() -> () {
    let curry_placeholder: SmeltUnknown = SmeltUnknown::Symbol("Symbol(curry.placeholder)@10319");
    let _ = curry_placeholder;
}
```

Every read of `partial.placeholder` then answered `SmeltUnknown::Null`. A sentinel
that reads `null` is worse than a missing one: `partial(fn, placeholder, 'b',
placeholder)` filled the placeholder slots with a real argument instead of skipping
them, so the spec saw a plausible-but-wrong argument list rather than an error.

**Fix.** A module-scope `f.prop = <expr>` whose `f` resolves to a function item is
recorded as a **const item** under the compound key `f.prop`, and three read paths
resolve against it: the member read (`partial.placeholder`), destructuring
(`const { placeholder } = partial`, which the record-destructuring path cannot type
because the source is a *function*), and an importer's qualified alias. The last
one needed a small correction of its own: `alias_imported_item` did the qualified
`imported.member` aliasing only on its fallback path and returned early when the
export map resolved the name, so an imported `partial.placeholder` stayed
unresolved.

Reusing the const-item machinery rather than adding a parallel store is what makes
the imported case work at all — it travels the same item-visibility path as any
other module const.

**Collected as a prepass**, right after the function items are predeclared and
before any body is lowered. A function reading its OWN static is a forward
reference to a statement below its declaration — exactly what `partial`'s body
does — so source-order collection would leave that read unresolved. The runtime
test pins it.

**Identity** holds because the recorded initializers are site-stable: `Symbol('…')`
lowers to a string keyed by its source span and a bare `Map` to an interned
builtin namespace, so two inlined copies compare equal. That is the identity
contract module-level `const` bindings already have under const inlining; a
per-read-fresh initializer (`f.prop = {}`) would be misrepresented by both equally,
and giving it a single allocation needs lazily-initialized module statics — a gap,
recorded below rather than smuggled in.

**Measured:** 907 → 908, 0 newly failing (`partial supports placeholders`). The
other placeholder specs need separate features, listed under the gaps.

## Known gaps, all measured and none regressed by this pass

1. **The array-callback lowering path does not lift.**
   `[n].map(function step(v) { … step(v - 1) … })` goes through the stdlib
   callback lowering rather than `function_expression_value`, and its
   self-reference still lowers to `SmeltUnknown::Null`. That path lowers the body
   itself and needs the same reservation wired into it.
2. **A self-recursive nested function *declaration* emits Rust that does not
   compile.** `function outer() { function step(n) { … step(n - 1) … } }` lowers
   `step` to a callback local assigned after the closure is built, so the body
   references `step` before it exists:

   ```rust
   let _smelt_tmp_2 = ::std::rc::Rc::new(|closure_arg_0: f64| { … (step)(…) … });
   let step = _smelt_tmp_2.clone();
   ```

   Pre-existing and not reachable from any of the three measured corpora (all
   compile at zero errors), which is why it was invisible. The same lift fixes
   it, but widening the change to nested function declarations touches a much
   more heavily used path, so it is recorded rather than bundled.
3. **A capturing self-recursive function expression keeps the empty-object
   lowering.** The honest fix is closure conversion — lift with the captures as
   leading parameters and hand out a closure that binds them — or a
   late-initialized self-reference cell. Neither shape appears in the measured
   corpora.
4. **A static property on a *returned* function is still unresolved.**
   `curry(fn, 2).placeholder` — es-toolkit `curry` copies `placeholder` onto the
   curried function it returns, and `flow`/`flowRight`'s placeholder specs read it
   off that value. The returned value is a generated `CurriedFunction1` struct, so
   this is a property on a runtime function *object*, not on a module function
   item; defect 9's item-keyed lowering does not reach it.
5. **A static property whose initializer must be one allocation.**
   `f.prop = {}` inlines a fresh object per read. Site-stable initializers
   (`Symbol`, a builtin namespace, a literal) are unaffected, which covers every
   occurrence in the measured corpora. The fix is lazily-initialized module statics
   — the `Item::MutableGlobal` machinery generalized past its literal-initializer
   V1 constraint.
6. **`Function.prototype.length` on a typed callable folds statically.**
   `partial(fn, 'a').length` const-folds to the declared parameter count of the
   *type* (2) instead of reading the value's own arity (0, because `partialImpl`
   returns a rest-only function). Defect 7 fixed the erased read; the typed read is
   a separate fold.
7. **A lifted item lands in the crate root rather than its source module.**
   `body_module_names` maps bodies to Rust files through `module.items`, which a
   nested lowering cannot reach, so a lifted item's body has no module entry and
   is emitted into `main.rs`. It compiles and runs (a child module reaches its
   ancestors' private items), but the generated file layout no longer mirrors the
   source. Fixing it means a deferred `lifted_items` list drained into
   `module.items` after statement lowering — one more `ModuleBuilder` field,
   which the second architecture pass is actively trying to reduce.

## Tests

* `an_arguments_object_is_iterable` — the iteration door is emitted and the
  coercion consults it.
* `array_containment_projects_an_optional_union_receiver` — the containment
  receiver projects to its list arm and compares against the projected `Vec`.
* `math_round_uses_the_javascript_tie_rule` — the helper is emitted, `Math.round`
  routes through it and never uses `f64::round`, and `Math.floor` neither changes
  nor pulls the helper in. `emits_math_rounding_calls` (pre-existing) updated in
  the same shape.
* `an_assertion_overload_still_emits_its_call` — the call survives in the caller's
  body instead of being folded to its arguments.
* `a_function_reading_arguments_lowers_to_one_rest_parameter` /
  `a_function_not_reading_arguments_keeps_its_declared_arity` — the variadic
  rewrite fires exactly where `arguments` is read and nowhere else.
* `an_erased_callable_reports_its_function_length` — the registry is emitted, the
  erasure boundary records the arity, and the read consults it.
* `a_static_property_on_a_function_declaration_resolves` — both the member read
  and the destructured binding resolve to the recorded value instead of null.
* `tests/function_statics_runtime.rs` (`#[ignore]`d runtime tier) — the member
  spelling and destructuring are the SAME value, a non-symbol static resolves, two
  statics stay distinct, and a function recognizes its own sentinel (the forward
  reference the prepass exists for).
* `tests/arguments_arity_runtime.rs` (`#[ignore]`d runtime tier) — short, exact
  and long calls against one three-parameter function; a real trailing `undefined`
  still counted as an argument; the declared names still binding positionally; and
  `length` reporting the declared arity across erasure, unchanged by reading
  `arguments`, and `0` for a genuinely variadic function.
* `tests/math_round_runtime.rs` (`#[ignore]`d runtime tier) — the tie rule in both
  directions, non-ties, the large-magnitude pass-through, `-0` survival, and that
  `floor`/`ceil`/`trunc` are untouched. Confirmed to actually assert by flipping
  one expectation and watching it fail.
* `tests/union_receiver_runtime.rs` (`#[ignore]`d runtime tier) — both switch arms
  trim correctly and a non-matching array leaves the string alone, so a projection
  that always answered `true` fails too. The golden can prove the projection is
  emitted; only running it proves the right arm was selected.
* `a_self_recursive_named_function_expression_lifts_to_an_item` — the item is
  emitted, the recursion is a direct item call inside it, and the body builds no
  empty erased record.
* `a_named_function_expression_name_stays_out_of_module_scope` — a module-scope
  declaration of the same name keeps its own signature.
* `tests/named_function_expression_runtime.rs` (three cases, `#[ignore]`d runtime
  tier) — the recursion terminates with the right value, two same-named lifts
  keep their own bodies, and the expression name does not leak into module scope.
  String goldens prove the wiring; only running the program proves the arithmetic.

## Validation

* `cargo test` green on the whole workspace, including the two end-to-end goldens
  that had to absorb the new prelude helper
  (`27_optional_chains`, `29_callable_object` — the two examples whose generated
  crate emits the `arguments` block; they gain the helper line and nothing else).
* `cargo clippy` could not run in this environment: the pinned toolchain
  (`1.96.1`) has no `cargo-clippy` component installed here. Unrelated to these
  changes and unchanged by them.
* remeda regenerated and re-run at its CI-pinned ref: 1789 passed / 0 failed,
  unchanged.

## SmeltUnknown delta

Measured three ways against `blocker-logs/smelt-unknown-baseline-es-toolkit.json`
(baseline avoidable 35677), because the starting tree is already above it:

| Tree | Avoidable | vs baseline | vs previous row |
| --- | ---: | ---: | ---: |
| `4d15304` (`main`, start of this pass) | 35711 | +34 | — |
| after defect 1 (iterable `arguments`) | 35711 | +34 | **+0** |
| after defect 2 (function-expression lift) | 35776 | +99 | **+65** |
| after defect 3 (union containment) | 35776 | +99 | **+0** |
| after defect 4 (`Math.round`) | 35776 | +99 | **+0** |
| after defect 5 (assertion overload) | 35892 | +215 | **+116** |
| after defects 6-8 (variadic `arguments`) | **35616** | **-61** | **-276** |
| after defect 9 (function statics) | **35422** | **-255** | **-194** |

The **+34 is pre-existing** and documented in `estk-typed-array-views.md`: #192
deliberately left the typed-array construction erasure un-baselined until the
`Type::Host` variant retires it at source, so `main` is already flagged by the
CI ratchet.

**Defect 1 contributes +0 avoidable.** Its only visible movement is a
classification shift: the runtime-prelude category rises by 240 and the
legitimate-boundary category falls by the same amount, because the new prelude
helper lands inside the block the classifier attributes to the runtime prelude.

**Defect 2 contributes +65, and it is restructured erasure rather than new
erasure.** The values involved are es-toolkit's own `any`-typed `mergeWith`
customizer arguments; they were already erased inside the closure. What the lift
adds is the `item_function_closure_expression` wrapper at each of the five
reference sites — one `let _smelt_tmp_N: SmeltUnknown = mergeRecursively(…);`
line per wrapper — plus the item's own signature, where the closure previously
had `closure_arg_N: SmeltUnknown` parameters instead. No `SmeltUnknown` appears
at a boundary that was concrete before, and nothing was routed through a tag to
make the generated Rust type-check.

**Defect 5 contributes +116, and every one of them is a call that should always
have existed.** `invariant` has 56 call sites in `invariant_spec.rs`; each was
being dropped, and each now passes two erased arguments because the source's own
signature is `invariant(condition: unknown, message: string | Error)` — a genuine
`unknown` plus a union, erased at the call boundary. 56 × 2 plus the
implementation's own signature is the whole delta. Isolated by reverting only
`module_init.rs` and re-measuring: with just defects 1–4 the number is +99.

This one is *not* reclassifiable. The emitted shapes are
`SmeltUnknown::Bool(…)` / `SmeltUnknown::String(…)` argument coercions, which
appear everywhere and cannot be labelled a legitimate boundary by
`classify_line` without mislabelling thousands of unrelated lines. The honest
statement is the one above: the erasure is what the source's `unknown` parameter
demands, at calls that previously did not exist. Defect 4 contributes +0.

**Defects 6-8 remove 276, which is what closes the whole gap.** The variadic
rewrite deletes the padded-argument erasure at every erased call to an
`arguments`-reading function — the
`smelt_args.get(N).cloned().unwrap_or(SmeltUnknown::Null)` triplets — and replaces
it with one list forwarding. That is a genuine reduction in erasure, not a
reclassification: the arguments are no longer erased individually because they are
never unpacked in the first place.

Defect 9 removes a further 194: a `partial.placeholder` read was an erased
`SmeltUnknown::Null` at every site and is now the recorded const.

The net is **−255 against the baseline**, so the pass ends *below* where it
started, which also clears the pre-existing +34 that `main` has carried since
#192. Per the `SmeltUnknown enforcement` policy ("avoidable decreases re-snapshot
in the same commit") the es-toolkit baseline is re-snapshotted to 35422, and
`--fail-on-regression` passes against it. The examples-corpus hard invariant is
untouched: avoidable erasure there stays 0. The examples-corpus hard invariant
(`blocker-logs/smelt-unknown-baseline.json`, avoidable == 0) is untouched.
