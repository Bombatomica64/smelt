# es-toolkit final 45 — group HOST / TYPE PREDICATES

Read-only investigation (no cargo run). Everything below was re-verified against the
current generated crate at `third_party/es-toolkit/dist-smelt/src` and the current
transpiler sources; prior blocker-log claims are marked where they were confirmed or
corrected.

Prebuilt binary used for name filters only: `dist-smelt/target/debug/deps/es_toolkit_probe-60af05449d054ba8`.

---

## 1. `isBrowser_spec::test_isbrowser_should_return_true_in_browser_environment`

* Spec: `third_party/es-toolkit/src/predicate/isBrowser.spec.ts:7` — `expect(isBrowser()).toBe(true)`,
  under `// @vitest-environment happy-dom` (line 1).
* Source: `isBrowser.ts:22` — `return typeof window !== 'undefined' && window?.document != null;`
* Generated (`dist-smelt/src/isBrowser.rs`):

```rust
pub(crate) fn is_browser() -> bool {
    return false;
}
```

The fold is *correct for the compiled profile*: `crates/smelt-frontend-ts/src/lowering/ambient_globals.rs`
documents `window`: absent (DOM-only), and `guards.rs::describe/typeof` folds
`typeof window !== 'undefined' && …` to `false` (`is_typeof_window`, `guard_constant_truth`).
Making this test pass requires compiling this one spec file against a DOM profile in which
`window.document` exists — i.e. honoring the `@vitest-environment happy-dom` pragma and
modeling a DOM global object.

* Root layer: none (profile decision, `smelt-frontend-ts/src/lowering/ambient_globals.rs`).
* Shares root with: nothing else in the 45.
* **Verdict: (b) out of scope.** The prior conclusion in `estk-host-globals.md` is confirmed.
  The spec's own first line demands a browser host (`happy-dom`); Smelt's target profile is
  the deterministic non-DOM Node profile. Only defensible change would be a per-file
  DOM profile switch driven by the vitest pragma — a host capability, not a lowering fix.

---

## 2. `isBuffer_spec::test_isbuffer_should_return_true_for_buffer_instances`

* Spec: `isBuffer.spec.ts:6-7` — `const buffer = Buffer.from('test'); expect(isBuffer(buffer)).toBe(true)`.
* Source: `src/predicate/isBuffer.ts:24`
  `return typeof globalThis.Buffer !== 'undefined' && globalThis.Buffer.isBuffer(x);`
  where `globalThis` is the **imported shim** `import { globalThis } from '../_internal/globalThis'`.
* Generated (`dist-smelt/src/isBuffer_1.rs`, `is_buffer_462`) — three separate wrongs in six lines:

```rust
let smelt_logical: bool = true;                                   // (a) typeof fold
if smelt_logical {
_smelt_tmp_3 = SmeltRecord::from([]);                             // (b) global object = {}
_smelt_tmp_4 = SmeltUnknown::Object(SmeltObject::from_unknown_record(_smelt_tmp_3.clone()));
_smelt_tmp_5 = match _smelt_tmp_4.clone() { SmeltUnknown::Object(map) => smelt_get_object_field(&map, "buffer"), _ => SmeltUnknown::Undefined }.clone();   // (c) key "buffer", not "Buffer"
_smelt_tmp_6 = { … match smelt_source_value … if let Some(smelt_function) = smelt_function { … } else { /* default callback returning SmeltUnknown::Null */ } };
_smelt_tmp_7 = (_smelt_tmp_6)(&x);                                // -> Null -> falsy
```

so `is_buffer_462` returns `false` for every input, including a real `__smelt_buffer` record.

The spec side builds the buffer correctly enough for identity:

```rust
let _smelt_tmp_1: SmeltList<f64> = … vec![] …;                    // (d) 'test' -> 0 bytes
let _smelt_tmp_2: SmeltUnknown = smelt_reflected_construct("buffer", vec![ … ]);
```

Four distinct defects, in order of load:

**(b) the global-object value.** `src/_internal/globalThis.ts` is
`const globalThis_ = (typeof globalThis === 'object' && globalThis) || (typeof window === 'object' && window) || …`.
`dist-smelt/src/globalThis.rs` shows the frontend *does* recognize this and materializes
`SmeltRecord::from([("__smelt_global_object", true)])` — but the value is bound to a dead local
and the module exports nothing, and in `isBuffer.ts` the imported binding `globalThis` is not
recognized as a global alias at all: `guards.rs:651 is_ambient_global_alias` returns `false`
because `self.imports.is_imported_binding("globalThis")` is true (the doc comment on it names
this exact es-toolkit shim and deliberately declines). So `globalThis.Buffer` falls to the
generic erased-record member path and reads a field off a *fresh empty record*.
`estk-globalthis.md` states "the static-member spelling `globalThis.Array` normalizes to the bare
identifier" — that is true only for the **unshadowed** spelling; through the imported shim it does
not fire. Correction to that log.

**(a) the `typeof` fold is unsound.** `crates/smelt-frontend-ts/src/lowering/guards.rs:483-520`
(`unknown_typeof_comparison`, final `if kind_lit.value.as_str() == "undefined"` arm) does:

```rust
let matches_kind = self.type_matches_typeof(value_ty, "undefined");
let result = if …Inequality… { !matches_kind } else { matches_kind };
… ExprKind::Literal(Literal::Bool(result))
```

`type_matches_typeof` (`lowering/testing/matchers.rs:1512`) has no `Type::Unknown` arm, so it
answers `false`, and `typeof <erased> !== 'undefined'` folds to the constant `true` for **any**
`unknown`-typed operand. Note `new_expr.rs:2618-2632` (`typeof_expression`) already carries the
comment "Only fold when the type pins a single spelling" — this guard path violates that rule.

**(c) case-folding symbol collision.** `emitter/place.rs:158` renders the key with
`self.symbol_source_name(*field)`, which should be `"Buffer"`. It emits `"buffer"` because
`references.rs:15 intern_source_name` interns `camel_to_snake(name)` — `"Buffer"` and `"buffer"`
hash to the *same* `Symbol` — and `smelt-hir/src/symbol.rs:44 record` is last-writer-wins
(`self.names[idx] = Some(original.into())`). This crate is full of typed-array `.buffer` reads
(`clone.rs`, `cloneDeepWith_1.rs`), so `buffer` wins and every `X.Buffer` read is emitted as
key `"buffer"`. Latent, general, and independent of es-toolkit: any object with both `Foo` and
`foo`-shaped members loses one spelling.

**(d) `Buffer.from(string)` drops the bytes.** `lowering/stdlib/buffer.rs:127-160
buffer_from_call` documents it: non-`List` source ⇒ `buffer_empty_bytes`. Not load-bearing for
this assertion (only identity is read) but wrong.

* Root layer: frontend-ts. `lowering/guards.rs` (`is_ambient_global_alias`,
  `unknown_typeof_comparison`), `lowering/expr/references.rs` (`intern_source_name`),
  `smelt-hir/src/symbol.rs` (`Names::record`), `lowering/stdlib/buffer.rs` (`buffer_from_call`).
* Shares root with: **test 4** (`isEqualWith` buffers) — same `is_buffer_462` returning constant
  `false`.
* **Verdict: (a) general defect, fixable.**
  * Fix 1 (frontend, S): a module-level `const` whose initializer *folds to the global alias*
    (the existing `expr_is_global_alias` extended over the profile's own `typeof`/`&&`/`||`
    folds) records a global-object alias, and that fact is exported so an **imported** binding of
    it is a global alias too. `matchers.rs:2954` already does this for the bare
    `const g = globalThis;` shape; the missing piece is the folded chain and the cross-module
    carry. Then `globalThis.Buffer.isBuffer(x)` normalizes to `Buffer.isBuffer(x)` and reaches
    `lowering/stdlib/buffer.rs:233 buffer_is_buffer_call` (already correct: `__smelt_buffer`
    marker check). Regression test: a two-module fixture (`shim.ts` exporting a folded
    `globalThis` alias, `use.ts` doing `globalThis.Buffer.isBuffer(x)`) asserting the emitted
    body is the marker check, not a `smelt_get_object_field` read.
  * Fix 2 (frontend, S): in the `typeof … 'undefined'` arm of `unknown_typeof_comparison`, when
    the operand type is `Unknown | Union | TypeParam` emit `ExprKind::UnknownIs { kind: Undefined }`
    (negated as needed) instead of `Literal::Bool`. Regression test: `typeof (x as unknown) !== 'undefined'`
    must not appear as a constant in the emitted Rust.
  * Fix 3 (hir, S): make the source-name registry key-collision-proof — either intern member
    symbols by their *source* spelling (separate namespace from Rust idents) or refuse to
    overwrite a differing recorded name and mint a fresh symbol. Regression test: an object with
    both `Buffer` and `buffer` members round-trips both keys.
  * Fix 4 (frontend, S): `Buffer.from(string[, encoding])` UTF-8/latin1-encodes the string into
    the byte list instead of `buffer_empty_bytes`.
  * Size overall: **M** (four S changes; Fix 1 is the load-bearing one).

---

## 3. `isEqualWith_spec::test_isequalwith_should_treat_arrays_with_identical_values_but_different_non_index_properties_as_equal…`

* Spec: `isEqualWith.spec.ts:155-181`. `array1 = [1,2,3]` then
  `array1.every = array1.filter = … = null`; `array2 = [1,2,3]` then `array2.concat = … = null`;
  `expect(isEqualWith(array1, array2, noop)).toBe(true)` (line 181). JS answers `true`: both stay
  arrays, `getTag` is `[object Array]`, and `areObjectsEqual`'s array arm compares only `length`
  and the indexed elements (`isEqualWith.ts:240-267`).
* Generated (`dist-smelt/src/isEqualWith_spec.rs:1168-1169`):

```rust
match &mut array1 { SmeltUnknown::Object(map) => { map.insert("every".to_owned(), SmeltUnknown::Null); },
  other => { *other = SmeltUnknown::Object(SmeltObject::new(Vec::from([("every".to_owned(), SmeltUnknown::Null)]))); } }
match &mut array2 { SmeltUnknown::Object(map) => { map.insert("concat".to_owned(), SmeltUnknown::Null); },
  other => { *other = SmeltUnknown::Object(SmeltObject::new(Vec::from([("concat".to_owned(), SmeltUnknown::Null)]))); } }
```

`array1`/`array2` are `SmeltUnknown::Array`, so the `other` arm fires and **replaces the array
with a one-key object**, destroying the three elements. The comparison then runs over
`{every: null}` vs `{concat: null}` → `false`. The prior log's claim ("this replaces the array
with an object") is **confirmed**, verbatim in the emitted `other =>` arm.

Two supporting observations:

* the chained assignment `array1.every = array1.filter = … = null` emitted **only one** insert
  (`"every"`); the other eight targets are dropped. Separate frontend defect (chained
  assignment lowering keeps only one place); does not change this test's expected answer.
* the same hole exists in the runtime helper, `crates/smelt-codegen-rust/src/lib.rs:3040-3050
  smelt_index_assign`:
  `SmeltUnknown::Array(array) => { if let Ok(index) = key.parse::<usize>() { array.set_index(index, value); } else { *target = SmeltUnknown::Object(…) } }`.
* Root layer: runtime representation + emitter. `crates/smelt-runtime` (`SmeltArray` has no
  non-index property store), `crates/smelt-codegen-rust/src/emitter/control_flow.rs:505-530`
  (erased static-member store), `lib.rs::smelt_index_assign`.
* Shares root with: `merge_spec::test_merge_should_behave_like_recursive_object_assign…`
  (`expect(Array.isArray(merge(['1'], { a: 2 }))).toBe(true)`, merge.spec.ts:129) — same
  "array + named property" model gap. Probably also
  `remove_spec::should_handle_sparse_arrays_correctly`.
* **Verdict: (a) general defect, fixable.** JavaScript arrays *are* objects with named
  properties; the honest model gives `SmeltArray` a side store for non-index keys (kept out of
  `len()`/iteration, included in `Object.keys` after the index keys, invisible to the array-tag
  equality arm). Changes: `smelt-runtime` `SmeltArray` (+`smelt_index_assign`, the erased
  member-store emitter, `smelt_get_object_field`-equivalent read for arrays, key enumeration in
  `emitter/map.rs`, `smelt_unknown_structural_eq`). Regression tests: (i) `a = [1,2,3]; a.x = 1;`
  ⇒ `Array.isArray(a)`, `a.length === 3`, `a.x === 1`, `Object.keys(a) == ["0","1","2","x"]`;
  (ii) two arrays with equal elements and different named props compare equal under the
  array-tag arm. Size: **L**.

---

## 4. `isEqualWith_spec::test_isequalwith_should_compare_buffers_when_customizer_returns_undefined`

* Spec: `isEqualWith.spec.ts:566` — `expect(isEqualWith(buffer, new Uint8Array([1]), noop)).toBe(false)`.
* Why JS says `false`: `isEqualWith.ts:252-255`, inside the `uint8ArrayTag` arm —
  `// Buffers are also treated as [object Uint8Array]s.` / `if (isBuffer(a) !== isBuffer(b)) { return false; }`.
* Generated: both operands are byte-buffer records
  (`smelt_reflected_construct("buffer", …)` vs `smelt_reflected_construct("uint8array", …)`,
  `isEqualWith_spec.rs:3906/3930`), and `smelt_object_to_string_tag` deliberately reports
  `[object Uint8Array]` for `__smelt_buffer` (`smelt-stdlib/src/host_object.rs:328-334` documents
  this, correctly). The discriminator is therefore `isBuffer`, which is
  `is_buffer_462` — the constant-`false` function quoted in §2. `false !== false` is `false`, the
  arm falls through to the element loop, both hold one `1`, so the answer is `true`.
* Root layer: identical to §2 (frontend-ts, `globalThis` alias + `typeof` fold).
* Shares root with: **test 2**. Fixing §2 Fix 1 fixes this test with no further work
  (`Buffer.isBuffer` already resolves through `__smelt_buffer`, and the two operands carry
  different markers).
* **Verdict: (a) general defect, fixable — same fix as §2.** Size: **S** (rides on §2).

---

## 5. `isError_spec::test_iserror_should_return_true_for_subclassed_values`

* Spec: `isError.spec.ts:10-11` — `class CustomError extends Error {}` / `expect(isError(new CustomError())).toBe(true)`.
* Source: `src/predicate/isError.ts:15` — `return value instanceof Error;`
* Generated callee (`dist-smelt/src/isError_1.rs`):

```rust
pub(crate) fn is_error_534(value: SmeltUnknown) -> bool {
    let _smelt_tmp_1: bool = matches!(value.clone().clone(), SmeltUnknown::Object(value) if value.contains_key("__smelt_error"));
    return _smelt_tmp_1;
}
```

* Generated caller (`isError_spec.rs:29`), the erasure of the `CustomError` instance:

```rust
… smelt_object_entries.push(("message", …)); … push(("stack", …)); push(("cause", …)); push(("name", …)); push(("custom", …));
smelt_object_entries.push(("__smelt_class".to_owned(), SmeltUnknown::String("CustomError".into())));
```

No `__smelt_error` entry, so the marker probe misses. Contrast the direct `new Error()` erasure
one test above, which *does* carry `("__smelt_error", String("Error"))`.

Cause: `crates/smelt-codegen-rust/src/emitter/coercion.rs:1974 host_base_markers` stamps a base
chain's marker only via `smelt_stdlib::host_object_by_class(base_name)`, and `Error` is **not** a
`HOST_OBJECTS` registry entry (`smelt-stdlib/src/host_object.rs:255-375` — errors are modeled
separately with the `__smelt_error: "<ClassName>"` convention). `emitter/call.rs:2569` even
asserts the opposite: "A user class `extends Error` carries `__smelt_class` and resolves through
the class path before reaching here" — false when the value has already crossed an erasure seam
into a `SmeltUnknown` parameter, which is exactly `isError(value: unknown)`.

* Root layer: codegen-rust emitter. `emitter/coercion.rs::host_base_markers` (+ the error-arm
  comment in `emitter/call.rs::instance_of_text`).
* Shares root with: `clone_spec::test_clone_should_clone_custom_classes` and
  `withTimeout_spec` (whose thrown value is `{… __smelt_domexception: true, __smelt_class: "TimeoutError" }`
  — the DOMException base marker *is* stamped there, showing the registry path works and only the
  error base chain is missing) — related, not identical.
* **Verdict: (a) general defect, fixable.** Extend the base-chain walk in `host_base_markers` so
  a class whose chain reaches a **builtin error** class contributes
  `("__smelt_error", SmeltUnknown::String("<nearest builtin error base>"))` — the builtin base
  name, not the user class name, so `class MyTypeError extends TypeError` satisfies both
  `instanceof Error` (`contains_key`) and `instanceof TypeError` (the recorded-name equality arm
  already in `instance_of_text`), while `instanceof MyTypeError` keeps resolving through
  `__smelt_class`. Drive the "is a builtin error class" test off the same list
  `instance_of_text` already uses (`Error | EvalError | RangeError | … | AggregateError`) rather
  than a new hand list. Regression test: `class C extends Error {}` erased to `unknown` answers
  `true` for `instanceof Error`, `false` for `instanceof TypeError`, and `getTag` `[object Error]`.
  Size: **S**.

---

## 6 & 7. `isFile_spec::test_isfile_returns_true_if_the_value_is_a_file` and `…can_be_used_with_typescript_as_a_type_predicate`

* Spec: `isFile.spec.ts:7-16` `beforeAll` sets
  `globalThis.File = class File extends Blob { name: string; constructor(chunks, filename, options) { super(chunks, options); this.name = filename; } }`;
  then `:24-25` `new File(['content'], 'example.txt', …)` / `expect(isFile(file)).toBe(true)`, and
  `:41-43` `items.filter(isFile)` must have length 2.
* Source: `isFile.ts:23-27` — `if (typeof File === 'undefined') return false; return isBlob(x) && x instanceof File;`
* Generated callee (`dist-smelt/src/isFile.rs`):

```rust
let _smelt_tmp_1: bool = SMELT_HOST_OVERRIDE_FILE.with(smelt_host_override_present);   // true
…
let _smelt_tmp_3: bool = is_blob(x.clone());                                            // true
_smelt_tmp_5 = matches!(x.clone().clone(), SmeltUnknown::Object(value) if value.contains_key("__smelt_file"));   // FALSE
```

* Generated caller (`isFile_spec.rs:28`), the erased override constructor's return value:

```rust
smelt_object_entries.push(("name".to_owned(), SmeltUnknown::String(smelt_object_value.name.into())));
smelt_object_entries.push(("__smelt_blob".to_owned(), SmeltUnknown::Bool(true)));
smelt_object_entries.push(("__smelt_class".to_owned(), SmeltUnknown::String("File".into())));
```

So `isBlob` passes (`__smelt_blob` is stamped, via `host_base_markers` seeing
`extends Blob`) and `instanceof File` fails: the value carries the *base's* marker and the
class name, never `__smelt_file`.

Answering the task's question directly — File/Blob **are** modeled as marker records, and the
native `new File(...)` path does stamp `__smelt_file`
(`codegen-rust/src/lib.rs:3029`, inside `smelt_blob_record_from_parts`). `isFile(file)` is false
because the spec's `file` is **not** a native File: it is an instance of the user's own
`class File extends Blob`, whose generated struct is
`struct File { name: String }` with `impl File::new` doing only `this.name = filename` (main.rs:6948/8168 —
the `super(chunks, options)` call and every Blob field are dropped). The remaining gap is
identity: `instance_of_text` compiles `x instanceof File` to a **static** marker probe
(`emitter/call.rs:2758-2787`, the `abort_marker`/`host_instance_markers` arm) and never consults
the override slot, even though `is_file`'s *presence* check one line above does
(`SMELT_HOST_OVERRIDE_FILE`). The comment at `coercion.rs:1886-1894` claims the erasure keeps
`instanceof` honest "including override classes assigned into a `globalThis.<Name>` slot" — true
for `instanceof Blob`, not for `instanceof File`.

* Root layer: codegen-rust emitter, `emitter/call.rs::instance_of_text` (host-marker arm) plus the
  class-constructor erasure in `emitter/coercion.rs` (no class identity is registered on the
  erased constructor function).
* Shares root with: each other; also the two `isBlob` environment-absence cases the Smelt.toml
  comment already documents.
* **Verdict: (a) general defect, fixable.** This is *not* a host capability gap: the whole
  host-override slot machinery already exists (`emitter/host_interop.rs`, `lib.rs:607-672`) and
  the write, the presence probe, and construction through the slot all work in this very test.
  Fix, in two general parts:
  1. When erasing a **class constructor** to `SmeltUnknown::Function`, register its class name
     (and base chain) next to the existing `smelt_link_function_identity` /
     `smelt_register_function_length` calls — a `smelt_register_function_class` side table.
  2. In `instance_of_text`, when the class name has a host-override slot in this crate
     (`stdlib::host_override_slot_names`), emit
     `smelt_host_override_instance_of(&SMELT_HOST_OVERRIDE_<NAME>, &x, &["__smelt_file", …])`:
     `Native` ⇒ today's marker probe; `Ctor(f)` ⇒ `x.__smelt_class` is `f`'s registered class or
     one of its registered subclasses; `Absent` ⇒ `false`.
     No name matching, no globalThis special-case: it is "instanceof reads the binding, and this
     binding lives in a slot".
  Regression test: a fixture that assigns `globalThis.File = class File extends Blob {}`, then
  asserts `new File(...) instanceof File` and `instanceof Blob` are both `true`, and that
  restoring the slot returns `instanceof File` to the native marker probe. Secondary (independent)
  fix worth filing: `class X extends <host object>` should run the host base constructor so the
  subclass instance actually carries the base's fields (`content`/`type`/`size`), instead of
  dropping `super(...)`. Size: **M**.

---

## 8. `isFunction_spec::test_isfunction_should_return_true_for_functions`

* Spec: `isFunction.spec.ts:8-9` — `const slice = Array.prototype.slice; expect(isFunction(slice)).toBe(true)`.
  Answer to the task's question: `slice` is `Array.prototype.slice`, a first-class reference to a
  builtin *prototype method*.
* Generated (`dist-smelt/src/isFunction_spec.rs:12-14`):

```rust
let _smelt_tmp_1: SmeltUnknown = smelt_builtin_namespace("Array");
let _smelt_tmp_2: SmeltUnknown = match _smelt_tmp_1.clone() { SmeltUnknown::Object(map) => smelt_get_object_field(&map, "prototype"), _ => SmeltUnknown::Undefined }.clone();
let slice: SmeltUnknown = match _smelt_tmp_2.clone() { SmeltUnknown::Object(map) => smelt_get_object_field(&map, "slice"), _ => SmeltUnknown::Undefined }.clone();
```

`smelt_builtin_namespace("Array")` builds
`{ __smelt_builtin_namespace: true, name: "Array", __smelt_call: … }`
(`codegen-rust/src/reflection_prelude.rs:315`) which has no `prototype` member, so
`_smelt_tmp_2` is `Undefined`, `slice` is `Undefined`, and
`is_function_536` (`matches!(value, SmeltUnknown::Function(_))`) is `false`.

* Root layer: frontend-ts. `lowering/expr/references.rs:818 builtin_namespace_value_expression`
  models the namespace object but not its members; nothing lowers
  `<Builtin>.prototype.<method>` as a value. Compare `lowering/stmt/assignments.rs:749
  object_static_function_member`, which already does exactly this job for
  `Object.keys`/`Object.fromEntries` etc. (arity + callable), proving the pattern is accepted.
* Shares root with: §2 defect (c)/(b) family in spirit (a member of a modeled builtin read
  dynamically yields `undefined`), and with any spec doing `Array.prototype.slice.call(...)`.
* **Verdict: (a) general defect, fixable.** Generalize the `Object.<fn>` first-class-member rule
  to any *recognized* builtin member reference, including the `<Builtin>.prototype.<method>`
  spelling: resolve `(builtin, member)` through the existing stdlib method-recognition registry
  (`smelt_stdlib::typescript_field_rule` / the per-builtin method tables the call path already
  uses) and lower the reference to a callable with the registry's arity, so
  `Array.prototype.slice` is a `SmeltUnknown::Function` and `.call(x)` dispatches through the
  existing `smelt_function_method` path (`emitter/place.rs:140`). A member with no registry entry
  stays `undefined` (honest). Regression tests: `typeof Array.prototype.slice === 'function'`,
  `(Array.prototype.slice as any).length`, and `Array.prototype.slice.call([1,2,3], 1)`.
  Size: **M**.

---

## 9. `isSymbol_spec::test_issymbol_returns_true_for_symbols`

* Spec: `isSymbol.spec.ts:11` — `expect(isSymbol(Symbol.iterator)).toBe(true)`. Lines 8-10
  (`Symbol()`, `Symbol('a')`, `Symbol.for('a')`) all pass.
* Generated (`dist-smelt/src/isSymbol_spec.rs:31`):

```rust
let _smelt_tmp_6: bool = is_symbol_550(SmeltUnknown::String("__smelt_symbol_iterator".into()));
```

against `is_symbol_550` = `matches!(value.clone(), SmeltUnknown::Symbol(_))` → `false`.
The three passing cases emit `SmeltUnknown::Symbol("Symbol()@281")` / `Symbol("Symbol(a)@324")` /
`Symbol("Symbol.for(a)")`, so the only broken case is the **well-known** symbol.

Cause: `crates/smelt-frontend-ts/src/lowering/stmt/assignments.rs:722 symbol_static_member`:

```rust
let key = crate::lowering::ty::computed_key_symbols::well_known_symbol_key(member.property.name.as_str())?;
let ty = self.ctx.krate.types.intern(Type::String);
Some(body.push_expr(Expr { kind: ExprKind::Literal(Literal::String(key)), ty, … }))
```

A well-known symbol in **value** position is lowered as the synthetic *property-key string*. That
is right for `obj[Symbol.iterator]` (key derivation) and wrong for every other use: it is a
`string`, so `typeof`, `isSymbol`, `getSymbols`, and symbol-vs-string equality all answer wrongly.

* Root layer: frontend-ts, `lowering/stmt/assignments.rs::symbol_static_member` +
  `lowering/ty/computed_key_symbols.rs::well_known_symbol_key`.
* Shares root with: **test 10** (`Symbol.toStringTag` as a computed key — the same key scheme,
  the complementary half of the bug).
* **Verdict: (a) general defect, fixable.** Split "the symbol's value" from "the key it indexes":
  `Symbol.<name>` lowers to `Literal::Symbol("Symbol(Symbol.<name>)")` (matching the existing
  `Symbol()`/`Symbol.for()` spelling scheme in
  `lowering/stdlib/call_dispatch.rs::registry_symbol_spelling`), and **one shared function** maps
  that spelling to the storage key `well_known_symbol_key(name)` — used both by the static
  computed-key declaration path and by the dynamic key coercion
  (`smelt_property_key`, `codegen-rust/src/lib.rs:3071`, which today maps
  `SmeltUnknown::Symbol(d)` to `__smelt_symbol:{d}` and must special-case nothing beyond
  consulting the same table). Regression tests: `typeof Symbol.iterator === 'symbol'`;
  `const s = Symbol.iterator; ({ [s]: 1 })[Symbol.iterator] === 1` (value and static key paths
  agree); `Object.getOwnPropertySymbols({ [Symbol.iterator]: 1 })` reports one symbol.
  Size: **M**.

---

## 10. `isPlainObject_spec::test_isplainobject_should_return_false_for_invalid_plain_objects`

* Spec: `isPlainObject.spec.ts:62` —
  `expect(isPlainObject({ [Symbol.toStringTag]: 'string-tagged' })).toBe(false)`.
  JS: `Object.prototype.toString.call(v)` consults `v[@@toStringTag]` and returns
  `"[object string-tagged]"` ≠ `"[object Object]"`.
* Generated value (`isPlainObject_spec.rs:299`):

```rust
_smelt_tmp_26 = SmeltRecord::from([("__smelt_symbol_to_string_tag".to_owned(), "string-tagged".to_owned())]);
```

(the key scheme is right here — the computed-key half of §9 works), and the predicate's last
statement (`isPlainObject_1.rs:60-62`):

```rust
_smelt_tmp_16 = smelt_object_to_string_tag(&(value.clone()));
_smelt_tmp_17 = _smelt_tmp_16 == "[object Object]".to_owned();
```

`smelt_object_to_string_tag` (emitted from `codegen-rust/src/lib.rs:2102`) checks every host
marker and `__smelt_builtin_namespace`, then falls through to `"[object Object]"`. It never looks
at `__smelt_symbol_to_string_tag`, so the tagged object reports `[object Object]` and
`isPlainObject` answers `true`.

* Root layer: codegen-rust runtime prelude, `crates/smelt-codegen-rust/src/lib.rs`
  (`smelt_object_to_string_tag`).
* Shares root with: §9 (same well-known-symbol modeling), and any spec asserting
  `Object.prototype.toString` on a tagged object.
* **Verdict: (a) general defect, fixable.** Per ES2024 §20.1.3.6, `@@toStringTag` (when it is a
  String) wins over the builtin tag, so add a **first** arm to the `SmeltUnknown::Object` branch:
  `if let Some(SmeltUnknown::String(tag)) = map.get(<well_known_symbol_key("toStringTag")>) { return format!("[object {tag}]"); }`
  — taking the key from the same shared table as §9 rather than hardcoding the spelling. Same for
  the `SmeltUnknown::Array` arm if a tagged array is ever produced. Regression test:
  `Object.prototype.toString.call({ [Symbol.toStringTag]: 'x' }) === '[object x]'` and
  `isPlainObject` of it is `false`, while an untagged `{}` is unchanged. Size: **S**.

---

## 11. `isPlainObject_spec::test_isplainobject_should_return_true_for_cross_realm_plain_objects`

* Spec: `isPlainObject.spec.ts:2` `import { runInNewContext } from 'node:vm'`; `:100`
  `expect(isPlainObject(runInNewContext('({})'))).toBe(true)`. The source comment at
  `isPlainObject.ts:53` states the third prototype clause exists solely
  "Required to support node:vm.runInNewContext({})".
* Generated (`isPlainObject_spec.rs:483-485`):

```rust
let _smelt_tmp_0: SmeltRecord<String, SmeltUnknown> = SmeltRecord::from([]);
let _smelt_tmp_1: SmeltUnknown = SmeltUnknown::Object(SmeltObject::from_unknown_record(_smelt_tmp_0.clone()));
let _smelt_tmp_2: SmeltUnknown = { let smelt_function_value = _smelt_tmp_1.clone(); … if let Some(smelt_function) = smelt_callable { … } else { SmeltUnknown::Null } };
```

The unresolved `node:vm` import becomes an empty record; calling a non-callable yields
`SmeltUnknown::Null`; `is_plain_object_527`'s first guard (`!value`) returns `false`.

* Root layer: none reachable — passing the assertion requires **evaluating the JavaScript source
  string `'({})'` in a fresh realm**, i.e. an embedded JS engine plus a second realm whose
  `Object.prototype` differs from this one.
* Shares root with: nothing (the `node:vm` import is unique to this spec).
* **Verdict: (b) out of scope.** `runInNewContext('({})')` from `node:vm` is precisely the
  "node:vm cross-realm" host capability named in the brief.
  One general defect *is* visible here and should be filed separately, because it is what turns an
  unsupported host into a silently wrong answer rather than a build error: an unresolved module
  import lowers to an **empty record** (`SmeltRecord::from([])`) and calling a non-callable
  erased value falls back to `SmeltUnknown::Null` instead of throwing a JS `TypeError`. Making
  the unresolved import an honest blocker (or the call throw) would surface this spec as
  unsupported instead of as a false assertion. Size of that hygiene fix: **S**; it does not make
  this test pass.

---

## Summary

| test | root family | verdict | size |
| --- | --- | --- | --- |
| `isBrowser::…true_in_browser_environment` | DOM host profile (`@vitest-environment happy-dom`) | out of scope | — |
| `isBuffer::…true_for_buffer_instances` | `globalThis` alias through an imported shim + unsound `typeof <unknown> !== 'undefined'` fold (+ `Buffer`/`buffer` symbol collision, `Buffer.from(string)` bytes) | general defect | M |
| `isEqualWith::…non_index_properties…` | `SmeltUnknown::Array` has no non-index property store; named store replaces the array with an object | general defect | L |
| `isEqualWith::…compare_buffers…` | same as `isBuffer` (constant-`false` `is_buffer_462`) | general defect | S (rides on isBuffer) |
| `isError::…true_for_subclassed_values` | `host_base_markers` does not stamp `__smelt_error` for a class whose base chain reaches a builtin error | general defect | S |
| `isFile::…returns_true_if_the_value_is_a_file` | `instanceof <host name>` ignores the host-override slot; erased class ctor carries no class identity | general defect | M |
| `isFile::…as_a_type_predicate` | same as above | general defect | M (same fix) |
| `isFunction::…true_for_functions` | builtin namespace object exposes no `prototype`/method members as values | general defect | M |
| `isSymbol::…true_for_symbols` | `Symbol.<wellKnown>` in value position lowers to the key *string*, not a symbol | general defect | M |
| `isPlainObject::…false_for_invalid_plain_objects` | `smelt_object_to_string_tag` ignores `@@toStringTag` | general defect | S |
| `isPlainObject::…cross_realm_plain_objects` | `node:vm` `runInNewContext` (JS eval + second realm) | out of scope | — |

Two of eleven are genuine host-capability gaps (DOM, `node:vm`). The other nine are general
lowering/runtime defects; the cheapest wins are `@@toStringTag` (S), the error base marker (S),
and the `typeof <unknown>` fold (S), and the highest-leverage single fix is the global-alias
propagation, which clears two tests (`isBuffer`, `isEqualWith` buffers).
