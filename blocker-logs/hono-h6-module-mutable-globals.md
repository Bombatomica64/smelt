# Hono family H6 — mutable module globals beyond literal primitives

Probe: `smelt --manifest-path third_party/hono/Smelt.toml check --message-format json`
at `honojs/hono@eebdf7be39abf0a872671835ccce0c4f03ea497a`.
1 occurrence, 1 file. **Partially landed** — see §7.

## 1. The site

`src/router/reg-exp-router/router.ts:21`:

```ts
let wildcardRegExpCache: Record<string, RegExp> = createNullObject()

function buildWildcardRegExp(path: string): RegExp {
  return (wildcardRegExpCache[path] ??= new RegExp(/* … */))     // line 23
}
// …
  wildcardRegExpCache = createNullObject()                       // line 140
```

Three distinct demands in three lines: a **non-literal initializer** (a call), a
**non-primitive type** (`Record<string, RegExp>`), and a **write through** the
binding (`cache[path] ??= …`).

## 2. Wrong output

Lowering rejected with

```text
module-level mutable binding initializer must be a literal for now
```

Line 140 (a whole-binding reassignment from inside a method body) is what
classifies the binding as a mutable global at all; the initializer then failed
the V1 constraint.

## 3. Responsible functions

`ModuleBuilder::register_mutable_global_decl`,
`crates/smelt-frontend-ts/src/lowering/module_init.rs:1049`, and the two V1
constraints documented in `blocker-logs/estk-module-globals.md`:

* `mutable_global_literal_init` accepted only a number/string/boolean literal.
* `mutable_global_type_is_primitive` accepted only `Float | Int | Bool |
  String`.

and downstream, `emit_mutable_globals` (`crates/smelt-codegen-rust/src/lib.rs`)
which returned an `EmitError` for any other type, plus `global_get_text` /
`global_set_text` (`emitter/host_interop.rs`) which chose `Cell` for everything
except `String`.

## 4. Design — what landed

### The initializer

The classification pass runs **before** imports and function items resolve
(see the call order in `ModuleBuilder::program`: `collect_mutable_globals` sits
above `predeclare_function_items`), so `createNullObject()` cannot be lowered
there — its callee would resolve to an erased import placeholder. But the
initializer is not needed early; only the *reads and writes* are.

So it is two-phase:

* The classification pass registers the item with
  `MutableGlobalInit::Pending`, taking the type from the annotation.
* The module body already **recognizes and skips** the lifted binding's own
  declarator (`is_lifted_global_declarator`, used at
  `lowering/testing/matchers.rs`). That is where the initializer is lowered —
  by then imports and items are resolved — into a synthesized **nullary
  function item** returning the expression, and the global's `init` becomes
  `Initializer(item)`.
* A `Pending` initializer reaching MIR is a **compiler bug**, not a source
  problem, and `lower_globals` reports it as one rather than defaulting the
  cell to a value the source never wrote.

Going through a real function item means the expression reaches codegen by the
ordinary path: `assign_function_ids` gives it a `FuncId`, `lower_item_functions`
lowers it, the emitter emits it, and the cell's initializer only has to call it:

```rust
thread_local! {
    static SMELT_GLOBAL_CACHE_0: ::std::cell::RefCell<SmeltRecord<String, String>> =
        ::std::cell::RefCell::new(smelt_global_init__cache__module_….());
}
```

A `thread_local!` initializer is an arbitrary expression run once per thread on
first access, which is the same guarantee JavaScript gives ("module state is
initialized before any consumer runs") per generated test thread — the property
the V1 design already relied on for literals.

One subtlety cost a panic before it was found: lowering into a **fresh** `Body`
while the module scope still holds module-body locals made name resolution hand
back `LocalId`s that index nothing (`local_ty` panicked with "local id should
point to an existing local"). The scope is now emptied for the duration, which
is also the correct *rule*: the initializer is a separate function, so a
module-body local is genuinely not in scope there, and referencing one now
reports an unresolved name instead of mis-resolving.

### The type

`Cell` only works for `Copy` values — a `Cell::get` moves the value out. So the
cell type follows the value:

| value type | cell | read | write |
| --- | --- | --- | --- |
| `Float`/`Int`/`Bool` | `Cell<f64 \| i64 \| bool>` | `.with(Cell::get)` | `.with(\|v\| v.set(x))` |
| everything else | `RefCell<T>` | `.with(\|v\| v.borrow().clone())` | `.with(\|v\| *v.borrow_mut() = x)` |

The three decisions — cell type, read spelling, write spelling — were made in
three separate places and are now driven by one predicate
(`global_uses_copy_cell`), because a mismatch between any two of them is a
compile error in the generated crate rather than a diagnostic. The `const`
block is kept only where it still applies (a `Copy` primitive with a literal
initializer), since neither an owned nor a computed initializer can be `const`.

## 5. Generality

The rules are "a mutable global's initializer may be any expression, lowered as
a nullary initializer function" and "a non-`Copy` value lives in a `RefCell`".
Neither mentions a library, a type name, or an initializer spelling. The
date-fns `let defaultOptions: DefaultOptions = {}` shape — which had its own
pre-existing test asserting the blocker — now lowers, and that test became a
positive one.

## 6. What is deliberately still rejected

* **A non-literal initializer with no type annotation** (`let counter =
  seed()`): the pass order leaves nothing to type the cell with. Named blocker.
* **A write through a non-`Copy` global** — see §7.

## 7. NOT landed: writing through a global (this is the Hono site)

`cache[key] = value` mutates the value the cell **holds**. A `GlobalGet` yields
a *copy* of that value, so the write lands on the copy and is lost:

```rust
let mut _smelt_tmp_2: SmeltRecord<String, String> =
    SMELT_GLOBAL_CACHE_0.with(|value| value.borrow().clone());
_smelt_tmp_2.insert(key.clone(), value.clone());   // never written back
```

This shape was **unreachable before** — a primitive cannot be indexed — so
lowering it now would be *introducing* a silent wrong value, the worst outcome
available. It is therefore a named blocker instead:

```text
module-level mutable binding `wildcardRegExpCache` is written through
(`wildcardRegExpCache[key] = …` or `wildcardRegExpCache.field = …`); only
whole-value reassignment of a non-primitive mutable global is lowered
```

Detection is by a second set in `MutatedNameCollector`
(`mutated_through`), recording the root identifier of any member/index
assignment target, alongside the existing set of reassigned names.

**Proposed follow-up (not implemented).** A read-modify-write desugar, in the
same place the existing global desugars live
(`try_global_assignment_expression`, `lowering/stmt/assignments.rs`):

```text
g[k] = v        →   tmp = GlobalGet(g); tmp[k] = v; GlobalSet(g, tmp)
g[k] ??= v      →   same, with the logical-assign computed into tmp
```

The pieces exist — the postfix-update path in that file already pushes a local
and emits a side statement — but pointing the *ordinary* index-write lowering at
the temporary (rather than at another `GlobalGet`) needs the binding name
rebound to the temp local for the duration, and `wildcardRegExpCache[path] ??=
…` is used in expression position, so the desugar must also evaluate to the
stored value. That is careful work I did not want to land unverified; the
blocker keeps the corpus honest until it is done.

**Residual risk, stated plainly:** the blocker covers member and index
assignment targets. A *mutating method call* on a non-`Copy` global
(`names.push(x)`, `cache.set(k, v)` on a `Map`-typed global) would lose its
effect the same way and is **not** currently detected. No Hono, es-toolkit,
remeda or radash global has that shape, but it is a hole in the guard, not a
proof of safety.

## 8. Tests

* `crates/smelt-frontend-ts/src/tests/module_globals_tests.rs` — four tests
  replacing the two V1-constraint blocker tests:
  `an_unannotated_non_literal_initializer_is_a_named_blocker`,
  `an_annotated_non_literal_initializer_lowers_through_an_initializer_item`
  (asserting the global carries an `Initializer` item, so neither a literal nor
  the `Pending` placeholder can pass), `a_non_primitive_type_lowers`, and
  `writing_through_a_non_primitive_global_is_a_named_blocker`.
* `crates/smelt-frontend-ts/src/tests/part02_tests.rs` —
  `lowers_module_mutable_default_options_accessors` turned from a
  blocker assertion into a positive one.
* `crates/smelt-codegen-rust/tests/module_global_shapes_runtime.rs` (new tier,
  in the `references` shard of `.github/workflows/runtime-tiers.yml`) — three
  fixtures that RUN: a record-typed global with a call initializer (the Hono
  shape minus the write-through), a list-typed one, and a program holding all
  four cell kinds at once (`Cell<f64>`, `Cell<bool>`, `RefCell<String>` from a
  literal, `RefCell<String>` from a call) so a desync between the cell type and
  the read/write spellings is a compile failure.

## 9. Result

1 occurrence -> 1, but the message changed from a vague V1 restriction to the
precise remaining gap, and two of the three demands in those three lines are
now lowered. `src/router/reg-exp-router/router.ts` still blocks, on
`wildcardRegExpCache[path] ??= …` alone.
