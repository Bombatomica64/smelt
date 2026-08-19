# es-toolkit: iterable `arguments` and self-recursive named function expressions

Measured against es-toolkit at the ref pinned in `.github/compat/libraries.json`
(`e008a281`) with the fixture manifest `.github/compat/es-toolkit/Smelt.toml`,
starting from Smelt `4d15304` (`main`, the merge of #192).

## Result

| Corpus | Before | After |
| --- | --- | --- |
| es-toolkit | 875 passed / 184 failed | **883 passed / 176 failed** |
| es-toolkit probe blockers | 0 | 0 |
| remeda | 1789 passed / 0 failed | **1789 passed / 0 failed** |

Two independent defects, found by reading the largest failing groups rather than
the individual specs.

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

## Known gaps, all measured and none regressed by this pass

1. **`arguments` does not see the actual argument count.** The erased-call ABI
   pads a short call up to the callee's declared arity, so
   `ary(fn, 2)('a','b','c','d')` invokes `fn('a','b')` and `arguments` reads
   `['a','b',null]` instead of `['a','b']`. This is what the remaining twelve
   specs from defect 1 now fail on. The actual argument vector *is* available at
   the erasure boundary (`|smelt_args: Vec<SmeltUnknown>|`) and is discarded by
   the fixed-arity call; making it reach the callee is an ABI change, because the
   callee's signature and every call site are emitted from the same MIR
   signature. Not a representation problem, so not folded into this pass.
2. **The array-callback lowering path does not lift.**
   `[n].map(function step(v) { … step(v - 1) … })` goes through the stdlib
   callback lowering rather than `function_expression_value`, and its
   self-reference still lowers to `SmeltUnknown::Null`. That path lowers the body
   itself and needs the same reservation wired into it.
3. **A self-recursive nested function *declaration* emits Rust that does not
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
4. **A capturing self-recursive function expression keeps the empty-object
   lowering.** The honest fix is closure conversion — lift with the captures as
   leading parameters and hand out a closure that binds them — or a
   late-initialized self-reference cell. Neither shape appears in the measured
   corpora.
5. **A lifted item lands in the crate root rather than its source module.**
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

The baseline is therefore **not** re-snapshotted, matching how #192 handled its
own delta: laundering the number by re-snapshotting would hide both this +65 and
the pre-existing +34. The honest reading is that the es-toolkit ratchet has been
above its baseline since #192 and needs the `Type::Host` work
(`blocker-logs/phase2-type-host-spec.md`) to come back down; this pass does not
make that worse in kind, only in count. The examples-corpus hard invariant
(`blocker-logs/smelt-unknown-baseline.json`, avoidable == 0) is untouched.
