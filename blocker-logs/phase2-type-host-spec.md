# Phase 2 implementation spec — `Type::Host`

Brief: `docs/architecture-plan-second-pass.md` finding **B** and the Phase 2 row of
its sequence table. Predecessor investigation: `blocker-logs/estk-typed-array-views.md`
("Verification of that delta after the merge, and the decision").

Written **without compiling anything** — two other agents held the build slots.
Every count below comes from reading source, from `git`-tracked baselines, or from
the already-generated corpus in `target/compat-repos/`. Claims that need a compile
or a corpus run to settle are collected in §6 and marked as such. Three files this
spec cites are being edited concurrently
(`crates/smelt-codegen-rust/src/{lib.rs,byte_buffer_prelude.rs,reflection_prelude.rs,thrown.rs}`,
`crates/smelt-runtime/`, `crates/smelt-frontend-ts/src/lowering.rs` + `lowering/**`),
so every line number here is the line as read on 2026-08-19 and should be
re-resolved by name before editing.

---

## 0 · The claim this phase has to make good on

Exit criterion (plan §3): **es-toolkit avoidable erasure ≤ 35,677 with typed
arrays retained; `Uint8Array.prototype.set` dispatches without the `Map`
collision; corpora ≥ current** (es-toolkit 875/184, remeda 1789/0, radash 3
pre-existing compile errors).

Current: avoidable 35,738 against the 35,677 ratchet (`+61`, un-laundered).

The debt is one shape family: `smelt_reflected_construct("<view>", vec![…])`
emitted for a statically-spelled `new Uint8Array([0, 1, …])`, plus the
`SmeltUnknown` locals that hold its result.

Measured on the generated corpus at `target/compat-repos/es-toolkit/dist-smelt/src`:

| Fact | Value |
| --- | ---: |
| Non-prelude `smelt_reflected_construct` call sites | **137** |
| Files containing them | 12 |
| `SmeltUnknown` tokens on those 137 lines | **179** |
| …of which are the `: SmeltUnknown =` local annotation | 29 |
| …of which are inside the *argument* erasure | **150** |

So the argument erasure alone is 150 avoidable tokens — 2.5× the 61 the phase has
to retire. That is the lever, and §4 explains why it is the lever rather than the
type annotation.

### 0.1 A correction to the obvious plan, up front

`Type::Host { class }` **on its own does not move the metric one token.** The
report counts `SmeltUnknown` *tokens per line*, classified by line
(`crates/smelt-transpiler/src/unknown_report.rs:583` `classify_line`,
`:596` `is_legitimate_boundary_line`). Today's dominant line is

```rust
let _smelt_tmp_3: SmeltUnknown = smelt_reflected_construct("uint8array", vec![{ let smelt_l = _smelt_tmp_2; SmeltUnknown::Array(SmeltArray::with_id(smelt_l.id(), smelt_l.into_iter().map(|value| SmeltUnknown::Number(value as f64)).collect::<Vec<_>>())) }]);
```
(`cloneDeep_spec.rs:676`) — 3 tokens. Retyping the destination to
`SmeltRecord<String, SmeltUnknown>` leaves 3 tokens: the annotation still names
`SmeltUnknown`, and the argument is still erased because
`host_construct_text` (`crates/smelt-codegen-rust/src/emitter/host_interop.rs:77-99`)
unconditionally calls `self.erase(arg)` on every argument.

The metric moves only when the **constructor stops taking `Vec<SmeltUnknown>`**.
`Type::Host` is what makes that possible — it is the type that lets the frontend
pick a *form-specific, typed* constructor entry from the argument's static shape
instead of pushing everything through one dynamic `(kind, Vec<SmeltUnknown>)`
door. So `Type::Host` is necessary and enabling, not sufficient; §4 and §5 stage
it accordingly.

---

## 1 · Site inventory

### 1.1 The `JsMap` precedent, traced

`Type::JsMap(TypeId, TypeId)` (`crates/smelt-hir/src/ty.rs:44`) is the exact
precedent: its doc comment (`ty.rs:32-43`) says the variant exists *solely*
because `Dict(String, V)` and `Map<String, V>` intern to the same `TypeId` and
"become indistinguishable", and that codegen reads it at "the one place spelling
matters".

Tracing what carrying it actually cost:

| Measure | `Type::JsMap` today |
| --- | ---: |
| Total `Type::JsMap` references in `crates/**/*.rs` | **98** |
| …`smelt-codegen-rust` | 52 |
| …`smelt-frontend-ts` | 38 |
| …`smelt-hir` | 3 |
| …`smelt-mir` | 3 |
| …`smelt-transpiler` | 2 |
| …`smelt-frontend-py` | **0** |

Structurally those 98 split three ways, and `Host` will split the same way:

1. **`Dict | JsMap` grouped arms** — the majority. `JsMap` rides along with `Dict`
   because they share machinery (`ty/assignability.rs:106-107`, `emitter/map.rs`
   ×13, `classes.rs` ×4, `lib.rs` ×3, `stubs.rs:431`). **`Host` gets none of
   these**: sharing an arm with `Dict` is exactly the failure mode
   `estk-typed-array-views.md` documents.
2. **Its own dedicated arm** where spelling is load-bearing — 8 sites:
   `emitter/coercion.rs:1072`, `:1455`, `:2193`; `emitter/types.rs:854`, `:1021`;
   `emitter/strings.rs:604`; `emitter/core.rs:3561`, `:3575`, `:3597`;
   `type_normalize.rs:96`; `format/types.rs:201`; `mir/format.rs:1528`;
   `mir/lower/generics.rs:108`; `ty/generics.rs:140`.
3. **`matches!` gates** that ask "is this a `Map`?" (`stdlib.rs:338`,
   `emitter/call.rs:2138`, `emitter/call_runtime.rs:571`, `list_mutation.rs:480`).

### 1.2 Must change — the 30 fully exhaustive `match`es on `Type`

Detected by a brace-balanced scanner over `crates/**/*.rs` that reports match
blocks naming all 20 `Type` variants with no catch-all arm. These are the sites
where adding a variant is a **hard compile error** and the compiler will list
them for you. Grouped by crate, with the enclosing function and what the `Host`
arm must do.

**`smelt-hir` — 2**

| Site | Function | `Host` arm |
| --- | --- | --- |
| `crates/smelt-hir/src/format/types.rs:183` | `type_text` | `format!("Host<{}>", class_name)` — debug formatting only |
| `crates/smelt-hir/src/type_normalize.rs:82` | `normalize_type` | identity: `types.intern(Type::Host { class })`, no children to normalize |

**`smelt-mir` — 2**

| Site | Function | `Host` arm |
| --- | --- | --- |
| `crates/smelt-mir/src/format.rs:1514` | `type_ref` | `format!("Host<{class}>")` |
| `crates/smelt-mir/src/lower/generics.rs:92` | `substitute_type_id` | identity — a host class carries no type arguments, so no substitution |

**`smelt-transpiler` — 2**

| Site | Function | `Host` arm |
| --- | --- | --- |
| `crates/smelt-transpiler/src/stubs.rs:338` | `ts_type` | the registry `class_name` verbatim (`"Uint8Array"`), which is the TS spelling |
| `crates/smelt-transpiler/src/stubs.rs:418` | `py_type` | `"typing.Any"` — Python has no equivalent; a stub must not claim one |

**`smelt-frontend-ts` — 5**

| Site | Function | `Host` arm |
| --- | --- | --- |
| `lowering/guards.rs:761` | `typeof_type_name` | `Some("object")` — every registry host object has `typeof === "object"`, *including* the boxed wrappers (`host_object.rs:150-156` documents this) |
| `lowering/stdlib.rs:1651` | `is_json_serializable_type_inner` | `false`. `JSON.stringify(new Uint8Array([1]))` is `{"0":1}`, not an array; claiming serializable would emit a wrong `serde_json` path |
| `lowering/stdlib/call_dispatch.rs:3127` | `overload_constraint_contains_unresolved_type_param` | `false` — no type parameters inside |
| `lowering/ty/annotations.rs:1548` | `concrete_type_requires_never_value` | `false` |
| `lowering/ty/generics.rs:125` | `substitute_type_params` | identity |

**`smelt-codegen-rust` — 19**

| Site | Function | `Host` arm |
| --- | --- | --- |
| `emitter/types.rs:761` | `type_text_with_scoped_type_params` | **the load-bearing one.** See §4 |
| `emitter/types.rs:990` | `default_value` | an empty host record of that class — `smelt_host_buffer_construct("<marker>", vec![])`; must NOT be `SmeltUnknown::Null` |
| `emitter/types.rs:250` | `optional_truthy_text` | always truthy: `{operand}.is_some()` (an object is truthy in JS) |
| `emitter/types.rs:319` | `value_truthy_text` | `"true"` |
| `emitter/coercion.rs:1031` | `erase` | `SmeltUnknown::Object(SmeltObject::from_unknown_record(({text}).clone()))` — the **aliasing** adapter, see §4.2 |
| `emitter/coercion.rs:1412` | `erase_value` | same |
| `emitter/core.rs:3381` | `type_contains_function` | `false` |
| `emitter/core.rs:3446` | `type_contains_unknown` | **`false`** — this is the arm that makes `Host` count as concrete. See risk R3 |
| `emitter/core.rs:3517` | `dict_uses_js_key_map` | `true` — a host object used as a dict key compares by identity, like `Class`/`Dict` |
| `emitter/strings.rs:583` | `string_like_operand_text` | `"\"[object <tag>]\".to_owned()"` from the registry's `to_string_tag`, mirroring the `JsMap` arm at `:604` |
| `emitter/union.rs:51` | `collect_union_type_params` | no-op |
| `emitter/union.rs:180` | `union_member_is_concrete` | **`true`** — a `Host` member has concrete Rust storage, so `Uint8Array \| null` can be a generated enum |
| `lib.rs:4200` | `type_contains_function` | `false` |
| `lib.rs:4246` | `type_supports_partial_eq` | `true` (`SmeltRecord` has `PartialEq`; see risk R5) |
| `lib.rs:4361` | `record_field_unknown_text` | the aliasing erase, same as `coercion.rs` |
| `classes.rs:135` | `type_param_in_dict_key` | `false` |
| `classes.rs:397` | `type_param_directly_inferable` | `false` |
| `classes.rs:456` | `type_param_in_callback` | `false` |
| `classes.rs:512` | `type_param_occurs` | `false` |

**`smelt-frontend-py` — 0.** No exhaustive `Type` match exists there. Python has
no host objects; the only Python-facing change is `py_type` in
`smelt-transpiler/src/stubs.rs:418`, above.

**Exact count: 30 must-change sites — 19 `smelt-codegen-rust`, 5
`smelt-frontend-ts`, 2 each in `smelt-hir` / `smelt-mir` / `smelt-transpiler`, 0
`smelt-frontend-py`.**

Scanner caveat: the same scan reported 16 further `catchall=False` blocks with
fewer than 20 variants named. I spot-checked five
(`emitter/types.rs:397` `place_ty`, `:578` `match_field_ty`,
`emitter/call.rs:1189`, `frontend-py/lowering/literals.rs:22`,
`frontend-py/lowering/pytest.rs:8`) and all five are matches on **other** enums
(`Place`, `Expr`, `StdlibClass`) that merely mention `Type::` inside. Treat the 30
as authoritative and `cargo check` as the real oracle.

### 1.3 MIR's own type handling

- **`crates/smelt-mir/src/types.rs`** (1,920 lines) is the `Rvalue` algebra.
  `Rvalue::HostConstruct { class_name: String, args: Vec<Operand> }` is at
  `:1694`. **No change required** for `Type::Host` — but §5 Stage 3 adds
  form-specific rvalue variants alongside it.
- **`crates/smelt-mir/src/lower/`**: `expr.rs:2051` lowers
  `ExprKind::HostConstruct` and propagates `expr.ty` unchanged, so a `Host`-typed
  HIR expression already produces a `Host`-typed MIR temp with zero edits.
  `place.rs:252` lists `HostConstruct` among rvalues that cannot be a place —
  unchanged. `lower/generics.rs:92` is in the must-change list.
- **`crates/smelt-mir/src/validate/`** — `assignment.rs` (378 lines),
  `operands.rs` (1,620), `closures.rs`, `structure.rs`, `mod.rs`: **zero
  references to `smelt_hir::Type`**. Validation walks the operand algebra
  (`for_each_operand`), not the type lattice, so Phase 2 needs no validator
  changes. This is the 2026-06 review item #6 paying off.
- **`crates/smelt-mir/src/erased_record_promote.rs`** — matches
  `Type::Unknown`/`Type::Dict` at `:143`/`:171`. Compiles unchanged. Must be
  reviewed: this pass promotes erased records, and a `Host` record must be
  *excluded* from promotion (its marker key set is not a program field set).

### 1.4 The emitter's type paths

| File | Role | Needed |
| --- | --- | --- |
| `emitter/types.rs` | `type_text_with_scoped_type_params:750`, `default_value:989`, plus 2 truthiness fns | 4 exhaustive arms (§1.2) |
| `emitter/coercion.rs` | `erase:1026`, `erase_value:1409` — the "one coercion seam" | 2 exhaustive arms + the un-erase direction (§4.2) |
| `emitter/map.rs` | 13 `Dict \| JsMap` destructures (`:25,87,129,220,270,412,487,757,766,812,835,856,889,984`) | **none must change; all must be verified not to accidentally admit `Host`.** They pattern-match `Dict`/`JsMap` explicitly, so a `Host` receiver falls out — which is the correct answer and the whole point |
| `emitter/host_interop.rs` | `host_construct_text:77`, `builtin_namespace_text:105` | the substantive rewrite (§4.1) |
| `emitter/call.rs` | `instance_of_text:2039` | ~7 erased-operand gates spelled `Some(Type::Unknown \| Type::TypeParam { .. } \| Type::Union(_) \| Type::Optional(_))`. Each becomes a *static* answer for `Host` (§2.3) |
| `emitter/union.rs` | `union_member_is_concrete:179` | one arm, and it flips `Host` to concrete |
| `emitter/strings.rs` | `string_like_operand_text:578` | one arm |

### 1.5 Compiles unchanged via a catch-all

The brace-balanced scan finds **415** `match` blocks over `Type` that *do* have a
catch-all arm (164 of them naming ≥4 variants):
`smelt-frontend-ts` 92 · `smelt-codegen-rust` 59 · `smelt-frontend-py` 13
(for the ≥4-variant subset). All compile unchanged. Most are correct by default —
a `Host` value legitimately is "not a list", "not a number".

The dangerous subset is the **erased-receiver gate**: `240` non-test sites match
the literal prefix `Some(Type::Unknown` (`smelt-codegen-rust` 157,
`smelt-frontend-ts` 79, `smelt-mir` 2, `smelt-transpiler` 2), and 12 of them use
the exact 4-way idiom `Some(Type::Unknown | Type::TypeParam { .. } |
Type::Union(_) | Type::Optional(_))`. Every one of those is a place where code
today says "the receiver is erased, so probe the runtime marker". A value that
used to be `Unknown` and is now `Host` **silently stops taking that path** and
falls through to whatever comes next — usually a concrete-class path that either
mis-answers or raises a blocker. This is the single largest correctness hazard in
Phase 2 and the reason §5 stages behind a fallback (Stage 2).

Densest files, for triage order: `emitter/coercion.rs` 22 ·
`emitter/core.rs` 16 · `emitter/call.rs` 13 · `emitter/strings.rs` 12 ·
`lowering/stdlib/call_dispatch.rs` 11 · `lowering/testing/matchers.rs` 10 ·
`emitter/optional_access.rs` 10.

### 1.6 Serde / on-disk format

`Type` derives `Serialize`/`Deserialize` (`crates/smelt-hir/src/ty.rs:6`), and
`TypeInterner` does too (`:113`). **There is no on-disk consumer.**

- `smelt-hir/Cargo.toml` and `smelt-mir/Cargo.toml` depend on `serde` but nothing
  in the workspace calls `serde_json::to_*` on a `Crate`, `Mir`, `Type`, or
  `TypeInterner` (grepped; the only `serde_json` uses in `smelt-transpiler` are
  the CLI schema, diagnostics, `probe`, and `unknown_report`).
- `smelt-schema.json` contains no `Type` variant names (0 hits for `Dict`/`JsMap`).
- The `SmeltUnknown` baselines (`blocker-logs/smelt-unknown-baseline*.json`)
  serialize *generated Rust text shapes*, not HIR types, so a new variant cannot
  perturb them except through the emitted output — which is the whole point.

Adding a variant to an externally-tagged serde enum is additive for readers of
old data anyway. **No golden-format impact. No migration.**

### 1.7 The variant's own shape — recommendation

The plan writes `Type::Host { class: Symbol }`. I recommend instead:

```rust
/// A JavaScript host object of a *statically known* registry identity.
Host {
    /// Index into `smelt_stdlib::host_object::HOST_OBJECTS`.
    class: smelt_stdlib::host_object::HostClass,
},
```

with a new `HostClass(u8)` newtype in `host_object.rs` carrying
`from_class_name(&str) -> Option<Self>`, `entry() -> &'static HostObject`, and
`Copy + Eq + Hash + Serialize + Deserialize`.

Why not `Symbol`:

- `Type` is interned by value and derives `Hash`/`Eq`; a `Symbol` works, but it is
  a per-`Crate` interner index, so the emitter must call `symbol_name()` and then
  re-look-up `host_object_by_class(name)` **at every one of the 30 arms**. A
  registry index makes `class.entry().marker` a `const`-ish read with no failure
  mode, which removes 30 `Result` paths.
- The registry is already the declared single source of truth
  (`host_object.rs:1-22`), and a `HostClass` index makes "this variant can only
  name a registry entry" true by construction rather than by convention — which
  is what stops the variant from becoming a general escape hatch.

Dependency check: `smelt-hir/Cargo.toml` depends only on `serde`;
`smelt-stdlib/Cargo.toml` depends only on `serde`. **No cycle.** Adding
`smelt-stdlib` to `smelt-hir` is a leaf-to-leaf edge.

If a reviewer prefers not to add that edge, the fallback is `class: Symbol` plus a
`host_class_of(&Crate, Symbol) -> Option<&'static HostObject>` helper in
`smelt-frontend-ts`, and the emitter resolving through `symbol_name`. Everything
else in this spec is unchanged either way.

---

## 2 · Which values move to `Host`, and which must not

The candidate list is `HOST_OBJECTS` (`crates/smelt-stdlib/src/host_object.rs:253-370`),
31 entries. The scope rule I applied: **a family moves only if (a) it is
currently `Type::Unknown` with no working concrete representation, and (b) moving
it retires measured ratchet debt or removes a method-surface collision.** A family
that already works stays where it is.

### 2.1 IN — the byte-buffer family (15 entries)

Exactly the entries with `byte_buffer: Some(_)`, which is a registry predicate,
not a list: `ArrayBuffer`, `SharedArrayBuffer`, the eleven typed-array views
(`Int8Array` … `BigUint64Array`), Node `Buffer`, `DataView`.

Why:

- They are the only registry entries that carry the ratchet debt. All 137
  non-prelude `smelt_reflected_construct` sites are byte-buffer constructions
  (12 files, 179 avoidable tokens).
- They are the only ones with a **method surface that collides**. `.set`,
  `.slice`/`.subarray`, `.buffer`, `.byteOffset`, `.byteLength`, `.length`,
  `.fill`, `.getFloat32`/`.setInt16` — `set`/`get`/`has`/`delete`/`clear`/`keys`/
  `values`/`entries` are the `Map`/`Set` surface, and `set` is the exact overlap
  that aborted the cheap fix.
- The construction sites are already funneled through one HIR node
  (`ExprKind::HostConstruct`, `crates/smelt-hir/src/expr/kinds.rs:730`) and one
  frontend entry (`byte_buffer_constructor_expression`,
  `crates/smelt-frontend-ts/src/lowering/new_expr.rs:1131`, which today interns
  `Type::Unknown` at `:1138` and stamps it at `:1150`). Changing one `intern` call
  types the whole family.
- Taking all 15 rather than only the element-bearing views is not a bigger change,
  it is a *smaller* one: `new Uint8Array(buffer, off, len)` takes an
  `ArrayBuffer`, `new DataView(clonedBuffer)` takes one, and `view.buffer` returns
  one. Splitting the family means those arguments and reads straddle the
  `Host`/`Unknown` line, and `view.buffer === buffer` becomes a cross-type
  comparison. One registry predicate, `entry.byte_buffer.is_some()`, is the clean
  cut.

**This is the recommended first cut, and it is the whole first cut.**

### 2.2 OUT — and why each already works

| Family | Registry entries | Current representation | Verdict |
| --- | --- | --- | --- |
| **Boxed primitives** | `Number`, `Boolean`, `String`, `Symbol` (`is_boxed_primitive`) | `boxed_primitive_constructor_expression` (`new_expr.rs:1444`) already builds a **typed** `ExprKind::DictLit` at `Type::Dict(String, Unknown)` (`:1461`) and then an explicit `ExprKind::UnknownCast` (`:1497`). The concrete step already exists; the erase is already an explicit boundary node | **OUT.** Contributes zero ratchet debt. Typing them `Host` would take `.valueOf()`/`.toString()` off the working dynamic path with nothing typed to replace it |
| **`RegExp`** | not in `HOST_OBJECTS` | `Type::Class { name: RegExp }` → `SmeltRegExp` (`emitter/types.rs:775` `is_regexp_class_symbol`). A real concrete Rust type | **OUT — leave alone.** Already better than `Host` would be |
| **`Date`** | not in `HOST_OBJECTS` | `__smelt_date` marker with dedicated `instanceof` (`emitter/call.rs:2095`) and dedicated numeric coercion (`emitter/types.rs:136,144,150,240`) | **OUT.** Working, and its numeric-coercion surface is wide; retyping it is its own project |
| **`Error` + subclasses** | `DOMException` only | dedicated `__smelt_error` records with a *class-name-carrying* marker and one-level hierarchy modelling (`emitter/call.rs:2052-2093`), plus the whole `thrown.rs` channel | **OUT.** Also concurrently being edited |
| **`Blob` / `File`** | `File`, `Blob` | `ExprKind::BlobFromParts` (`kinds.rs:700`) with a dedicated runtime builder | **OUT.** No debt, no method collision, and `File`'s double marker (`__smelt_file` over `__smelt_blob`) already encodes the subtype relation |
| **`SmeltJsMap` / `SmeltJsSet`** | not in `HOST_OBJECTS` | `Type::JsMap` / `Type::Set` — already lattice variants with concrete containers | **OUT.** They are the precedent, not the target |
| **`Proxy`, `Function`, `AbortController`** | not in `HOST_OBJECTS` | own constructors (`new_expr.rs:219,223,227`) | **OUT** |
| **Marker-only** | `WeakMap`, `WeakSet`, `Request`, `DOMException`, 8× `Intl.*` | `marker_only_builtin_marker` (`new_expr.rs:907`). Constructed then only `instanceof`-probed; `host_object.rs:1-22` documents that they have "no useful structural shape that source code reads" | **OUT of the first cut.** Zero debt, zero method surface. They are the *natural second cut* once the machinery exists, because a marker-only host object needs only the type and the `instanceof` fold — no method table at all |

So: **15 of 31 registry entries move; 16 stay.** Nothing that currently has a
working representation is disturbed.

### 2.3 A free win that falls out

With `Type::Host { class }`, `x instanceof Uint8Array` on a host-typed operand is
answerable **statically** from the registry (`class == target`, or
`class.entry().to_string_tag == target` for the Node-`Buffer`-is-a-`Uint8Array`
relation the registry already records). Today it is a runtime marker probe
(`emitter/call.rs` host-marker arms) built as a balanced `||` tree over 13 view
entries. Static folding removes that tree from every host-typed site — which is
pure avoidable-erasure reduction on top of the construction win, and is the same
mechanism `emitter/call.rs:2140` already uses for a concrete `JsMap`
(`if matches!(… Some(Type::JsMap(_, _))) { return Ok("true") }`).

---

## 3 · The method-dispatch resolution

### 3.1 What decides it today

Three files, in order:

1. **`crates/smelt-frontend-ts/src/lowering/stdlib/call_dispatch.rs:1142`** —
   `Self::dispatch_collection_method` is entry #37 of the ordered
   `builtin_call_handlers()` chain (`:1106`). The chain's doc comment says the
   order is load-bearing.
2. **`crates/smelt-frontend-ts/src/lowering/stdlib/collections.rs:70-76`** —
   **this is the receiver-kind decision**:
   ```rust
   let receiver_kind = match self.ctx.krate.types.get(effective_ty) {
       Some(Type::Dict(_, _) | Type::JsMap(_, _)) => {
           smelt_stdlib::TypeScriptReceiverKind::Map
       }
       Some(Type::Set(_)) => smelt_stdlib::TypeScriptReceiverKind::Set,
       _ => return Ok(None),
   };
   ```
   Gated upstream by the member-name filter
   `is_collection_method_name` (`collections.rs:179-184`), which contains
   `"set"`, `"get"`, `"has"`, `"delete"`, `"clear"`, `"keys"`, `"values"`,
   `"entries"`, `"add"`.
3. **`smelt_stdlib::recognition`** — `TypeScriptReceiverKind` has exactly two
   variants, `Map` and `Set` (`crates/smelt-stdlib/src/recognition.rs:20-25`);
   `TYPESCRIPT_METHODS` (`:108-140`) is 15 `(receiver, member, rule)` rows;
   `typescript_method_rule` (`:153`) is the lookup.

`TsMapMutation` then routes to `map_mutation_call`
(`collections.rs:411`), which re-destructures the receiver at `:427` and raises
the fatal blocker at **`collections.rs:436-441`**:
```rust
"set" => {
    let [key_argument, value_argument] = call.arguments.as_slice() else {
        return Err(SmeltError::unsupported(…, "Map.set requires key and value arguments"));
    };
```

### 3.2 `destView.set(srcView)` — before and after

Source: `target/compat-repos/es-toolkit/src/compat/object/clone.ts:99`, inside
the `dataViewTag` arm:
```ts
const clonedBuffer = new ArrayBuffer(byteLength);
const srcView  = new Uint8Array(buffer, byteOffset, byteLength);
const destView = new Uint8Array(clonedBuffer);
destView.set(srcView);
```

**Before, today (receiver `Type::Unknown`).** The receiver-kind match at
`collections.rs:71` falls to `_ => return Ok(None)`, the chain runs out, and the
call lowers as a *dynamic property read of `"set"` followed by an erased call*.
Generated `clone_1.rs:17-220`:
```rust
let src_view: SmeltUnknown;
let dest_view: SmeltUnknown;
…
_smelt_tmp_61 = { let smelt_source_value = match dest_view.clone() { SmeltUnknown::Object(map) => smelt_get_object_field(&map, "set"), _ => SmeltUnknown::Undefined }.clone().clone(); … } else { { let smelt_default_callback: … = ::std::rc::Rc::new(move |arg0: SmeltUnknown| -> SmeltUnknown { SmeltUnknown::Null }); smelt_default_callback } } };
_smelt_tmp_62 = (_smelt_tmp_61)(src_view);
```
The record has no `"set"` slot, so this resolves to the default callback and the
byte copy **silently never happens** — a wrong-answer bug, and one line carrying
~20 avoidable `SmeltUnknown` tokens.

**Before, with the rejected `Dict(String, Unknown)` typing.**
`collections.rs:71` matches the `Dict` arm → `TypeScriptReceiverKind::Map` →
`typescript_method_rule(Map, "set")` = `RuleId::TsMapMutation` →
`map_mutation_call` → `collections.rs:436` sees one argument, not two → the build
**aborts** with `Map.set requires key and value arguments`. This is the failure
`estk-typed-array-views.md` records.

**After, with `Type::Host { class: Uint8Array }`.**

1. `collections.rs:71`'s match sees `Type::Host` — neither `Dict`, `JsMap`, nor
   `Set` — and falls to `_ => return Ok(None)`. **`Map.set` can no longer claim a
   host receiver, structurally.** That alone removes the collision.
2. A new handler `Self::dispatch_host_object_method` is inserted into
   `builtin_call_handlers()` immediately **before** `Self::dispatch_collection_method`
   at `call_dispatch.rs:1142`. It reads the receiver type, requires
   `Some(Type::Host { class })`, and looks the member up in a new registry-derived
   table.
3. `Uint8Array` has `byte_buffer: Some(View)` and `element: Some(Uint8)`, so it
   presents the **typed-array-view capability**, whose `set` rule lowers to a
   byte-copy op: copy `srcView`'s decoded elements into `destView` at an element
   offset, encoding at `destView`'s own width — machinery the byte-buffer prelude
   already has (`byte_buffer_prelude::decode_expression`/`encode_expression`).

**A real `Map.set` is untouched.** Its receiver is still `Type::JsMap(k, v)`, so
`collections.rs:71` takes the same arm it takes today, `typescript_method_rule`
returns the same `TsMapMutation`, `map_mutation_call` builds the same
`ExprKind::DictSet` (`collections.rs:465`), and the emitter renders it through the
same `dict_set_text` (`crates/smelt-codegen-rust/src/emitter/map.rs:262`). The
diff for `Map.set` output must be **byte-empty**, and that is a checkpoint in §5.

### 3.3 The table, kept general

`TYPESCRIPT_METHODS` must not grow a per-class list — that would be exactly the
"per-class hack list" the standing rules forbid. Instead the receiver kind is
**derived from registry capabilities**:

```rust
/// What method surface a host object presents, derived from its registry entry.
pub enum HostCapability {
    /// `byte_buffer: Some(Storage)` — owns bytes, byte-addressed.
    ByteStorage,
    /// `byte_buffer: Some(View)`, `element: Some(_)` — a typed-array view.
    TypedArrayView,
    /// `byte_buffer: Some(View)`, `element: None` — `DataView`.
    ByteAddressedView,
    /// `byte_buffer: None` — identity only, no member surface.
    Opaque,
}
impl HostObject { pub const fn capability(&self) -> HostCapability { … } }
```

`TypeScriptReceiverKind` gains the same three non-`Opaque` variants, and
`TYPESCRIPT_METHODS` gains rows keyed on *capability*, never on class name. Node's
`Buffer` gets the `TypedArrayView` surface for free because its registry entry
already says `View` + `Uint8` — which is the correct answer (`Buffer` subclasses
`Uint8Array`) and is a rule, not a special case.

**Smallest first-cut member set.** Surveyed against the three corpora
(`target/compat-repos/*/src`), the byte-buffer member spellings that actually
appear are: `.length` (48 in es-toolkit, 3 radash), `.slice` (4 + 3),
`.byteLength` (5), `.byteOffset` (3), `.buffer` (3), `.set` (**1** — clone.ts:99).
`remeda` uses none. So the first cut is six members plus `.subarray`
(`stdlib.rs:2561` already pairs it with `slice`) and
`BYTES_PER_ELEMENT`. `DataView`'s `getX`/`setX` family appears in **no** corpus
and is deliberately deferred to the Stage-2 fallback.

---

## 4 · What the Rust type becomes

### 4.1 Recommendation: keep the record backing, change the constructor door

**Keep `SmeltRecord<String, SmeltUnknown>` as the Rust storage for a
`Type::Host` value.** Retiring the erasure does not require changing the runtime
representation, and changing it would be a much larger, riskier phase.

Concretely, `type_text_with_scoped_type_params` (`emitter/types.rs:750`) gains:
```rust
Type::Host { .. } => Ok("SmeltRecord<String, SmeltUnknown>".to_owned()),
```
This is byte-identical to what `Type::Dict(String, Unknown)` already emits, so
the *storage* question is settled by precedent and the structural-equality
question answers itself (§4.3).

The change that moves the metric is at the **constructor**, not the storage.
Today `host_construct_text` (`emitter/host_interop.rs:77-99`) does:
```rust
let arg_texts = args.iter().map(|arg| self.erase(arg)).collect::<…>()?;
let call = format!("{construct}({kind:?}, vec![{args}])", construct = …REFLECTED_CONSTRUCT, …);
self.value_at_type_text(&call, unknown_ty, dest_ty)
```
— every argument erased, unconditionally, because the runtime door is
`fn smelt_reflected_construct(kind: &'static str, args: Vec<SmeltUnknown>) -> SmeltUnknown`
(`reflection_prelude.rs:217`).

With `Type::Host` the frontend can pick the **JavaScript constructor form from the
argument's static HIR type** — a general rule over the lattice, not over class
names — and emit a typed door per form:

| Argument shape (HIR) | JS form | New runtime entry |
| --- | --- | --- |
| no arguments | `new X()` | `smelt_host_buffer_empty(marker) -> SmeltRecord<String, SmeltUnknown>` |
| `Int` / `Float` | `new X(n)`, n *elements* | `smelt_host_buffer_alloc(marker, n: f64) -> …` |
| `List(Float)` / `List(Int)` | element conversion | `smelt_host_buffer_from_elements(marker, elements: SmeltList<f64>) -> …` |
| `Host{c}` where `c.byte_buffer == Some(Storage)` (+ optional `Float`, `Float`) | byte **view** over shared storage | `smelt_host_buffer_view_of(marker, storage: SmeltRecord<…>, byte_offset: Option<f64>, length: Option<f64>) -> …` |
| `Host{c}` where `c` is a view | element conversion from another view | `smelt_host_buffer_from_view(marker, source: SmeltRecord<…>) -> …` |
| `Unknown` / `TypeParam` / `Union` | genuinely dynamic | **unchanged** `smelt_reflected_construct(kind, Vec<SmeltUnknown>)` |

The last row matters: `smelt_reflected_construct` **stays**, and stays reachable,
for the `new Object.getPrototypeOf(x).constructor(...)` path whose class is known
only at runtime. `estk-typed-array-views.md` already argues that path *is* a
legitimate boundary while the statically-spelled path is not; this design finally
lets the two be spelled differently, so the honest classification becomes
mechanical rather than a judgement call.

Each new entry is a thin wrapper over the existing
`smelt_host_buffer_construct` body (`byte_buffer_prelude.rs:480`), so the
"one constructor, direct and reflected records indistinguishable" invariant that
`reflection_prelude.rs:1-37` documents is preserved by construction: the wrappers
build their `Vec<SmeltUnknown>` **inside the prelude**, where the tokens are
classified `RuntimePrelude` and are correctly not program erasure.

Effect on the dominant line — `cloneDeep_spec.rs:676`:
```rust
// before: 3 avoidable SmeltUnknown tokens
let _smelt_tmp_3: SmeltUnknown = smelt_reflected_construct("uint8array", vec![{ let smelt_l = _smelt_tmp_2; SmeltUnknown::Array(SmeltArray::with_id(smelt_l.id(), smelt_l.into_iter().map(|value| SmeltUnknown::Number(value as f64)).collect::<Vec<_>>())) }]);
// after: 1 avoidable SmeltUnknown token (the annotation)
let _smelt_tmp_3: SmeltRecord<String, SmeltUnknown> = smelt_host_buffer_from_elements("__smelt_uint8array", _smelt_tmp_2);
```

### 4.2 Two new zero-copy adapters are **required**, not optional

This is the sharpest thing I found while reading the prelude, and it is a
correctness requirement, not an optimisation.

- **Erase (`Host` → `SmeltUnknown`) must share.**
  `SmeltObject::from_unknown_record` (`lib.rs:1091`) is
  `Self { id: record.id, values: record.values, order: record.order }` — it moves
  the `Rc`s, so the object and the record are the *same* backing store. That is
  what the existing `Dict(String, Unknown)` erase arm uses
  (`coercion.rs:1078`, `:1463`). The `Host` arms **must** use this and **must
  not** use `into_smelt_unknown`.
- **`IntoSmeltUnknown for SmeltRecord` copies.** `lib.rs:2976` builds
  `SmeltObject::with_id(self.id, self.iter()…collect())` — same `id`, **fresh
  `Rc`**. `===` (id equality) still holds, but a mutation through one side is
  invisible to the other.
- **Un-erase (`SmeltUnknown` → `Host`) has no sharing adapter today.**
  `SmeltFromUnknown for SmeltRecord` (`lib.rs:2838`) is
  `SmeltRecord::with_id_from_entries(object.id, object.iter()…)` — also a copy.

Byte-buffer semantics depend on aliasing: `estk-typed-array-views.md` §3 states
"an element write also lands **in place** in the backing `buffer` record, so
`view[0] = 1` is visible through `view.buffer` … (`view.buffer === buffer` keeps
holding)". A copying round trip breaks that for any write that *replaces* a
top-level field. So Phase 2 must add to the prelude:

```rust
impl SmeltRecord<String, SmeltUnknown> {
    /// Adopt a host object's backing store without copying, so a write through
    /// either view is observed through the other.
    fn from_unknown_object(object: SmeltObject) -> Self {
        Self { id: object.id, values: object.values, order: object.order }
    }
}
```
and an indexed-write entry `smelt_host_record_index_assign(&SmeltRecord<…>, key, value)`
(trivially available: `SmeltRecord::insert` already takes `&self`, `lib.rs:848`)
to replace today's `smelt_index_assign(&mut typed_array, …)` on the erased path
(seen at `cloneDeep_spec.rs:680`).

### 4.3 Structural equality between a directly-built and a reflectively-built record

es-toolkit's clone specs compare a reflectively-built clone against a
directly-built original with `toEqual`, i.e. structural equality. Generated form
(`cloneDeep_spec.rs:687`): `_smelt_tmp_8 = !(typed_array == _smelt_tmp_7);`

- Today both sides are `SmeltUnknown`, so `==` is `SmeltUnknown`'s `PartialEq`,
  which for `Object` routes to `smelt_object_structural_eq` with a cycle-`seen`
  set (`lib.rs:1255`).
- After, both sides are `SmeltRecord<String, SmeltUnknown>`, so `==` is
  `impl PartialEq for SmeltRecord` (`lib.rs:875`):
  `*self.values.borrow() == *other.values.borrow()` — a `HashMap` comparison whose
  element comparison recurses into `SmeltUnknown`'s `PartialEq`, which for the
  nested `bytes` / `buffer` fields lands back on `smelt_object_structural_eq`.

Both directions therefore compare the same field sets structurally, and the
"indistinguishable records" invariant is unaffected because §4.1 keeps **one**
constructor body. Two caveats:

- `SmeltRecord::eq` has **no cycle guard**. Byte-buffer records are acyclic today
  (a view holds `buffer`; a storage buffer holds no back-pointer), so this is safe
  *now* — but it is a latent hazard the moment anything adds a back-reference, and
  the regression test in §5 Stage 4 pins it.
- Identity comparisons already work: `SmeltRecord` implements `SmeltJsKeyEq` by
  `id` (`lib.rs:1058`), which is what `cloned_typed_array.clone().same_js_key(&typed_array.clone())`
  (`cloneDeep_spec.rs:682`) needs.

### 4.4 What I explicitly recommend against

**Do not introduce a `type SmeltHostRecord = SmeltRecord<String, SmeltUnknown>;`
alias to get the metric down.** It would work — the scanner is textual, so the
alias erases ~140 `SmeltUnknown` tokens from local annotations at a stroke — and
that is precisely why it must not be counted. It relabels rather than reduces, and
the standing rules forbid that. If the alias is wanted for readability, land it in
its **own** commit, report its delta separately, and label the delta cosmetic.

**Do not change the storage to `Vec<u8>` / `Vec<f32>` in this phase.**
`estk-typed-array-views.md` "Deliberately deferred" is right: it needs sub-`f64`
numeric types in HIR, which do not exist (`Type::Int`/`Type::Float` only). That is
Phase 2's successor, and `Type::Host` is its prerequisite.

---

## 5 · Staged plan

Each stage compiles and keeps all three corpora at or above current. Numbered
checkpoints are the gates; a stage that misses one gets reverted, not patched
forward.

### Stage 0 — registry plumbing (no behaviour change)

- `HostClass(u8)` newtype + `from_class_name` / `entry` / `capability` in
  `crates/smelt-stdlib/src/host_object.rs`.
- `HostCapability` derived from `byte_buffer` + `element`.
- `smelt-stdlib` added to `smelt-hir/Cargo.toml`.

**Checkpoint 0.** `cargo check --lib --no-default-features` clean. Corpora
untouched (nothing emits differently). Avoidable erasure: **unchanged, 35,738**.

**Regression test** (`host_object.rs` `mod tests`, alongside the existing
`byte_buffer_roles_are_classified`): `capability_is_derived_not_listed` — assert
`capability()` for all 31 entries is exactly the function of
`(byte_buffer, element)`, that the eleven views plus `Buffer` are
`TypedArrayView`, `ArrayBuffer`/`SharedArrayBuffer` are `ByteStorage`, `DataView`
is `ByteAddressedView`, and the remaining 16 are `Opaque`. A drift here silently
changes which method surface a class presents.

### Stage 1 — the variant, with nothing producing it

- `Type::Host { class: HostClass }` in `crates/smelt-hir/src/ty.rs`, with a doc
  comment in the `JsMap` house style stating the genuine distinction it preserves
  (`Dict` is shared between source `Map` and source `Record`, so a byte-backed
  host record has no way to be distinguished from a `Map` — cite
  `estk-typed-array-views.md`).
- All **30** must-change arms from §1.2.
- The `(Host{a}, Host{b})` arm in
  `lowering/ty/assignability.rs` — `a == b || a.entry().to_string_tag == b.entry().class_name`
  so `Buffer` is assignable to `Uint8Array`, which TypeScript declares and the
  registry already records. Without this arm the code compiles (it falls to
  `actual_ty == expected_ty` at `assignability.rs:151`) and is silently wrong.
- No frontend interns `Type::Host` yet.

**Checkpoint 1.** `cargo clippy --all-targets` clean; **all three corpora
byte-identical** to current output (nothing produces the variant, so any diff is a
bug in the 30 arms). Avoidable erasure: **unchanged, 35,738**.

**Regression test** (`crates/smelt-hir/src/ty.rs` `mod tests`):
`host_and_dict_intern_distinctly` — intern `Dict(String, Unknown)` and
`Host { class: Uint8Array }` and assert different `TypeId`s; then intern
`Host{Uint8Array}` and `Host{Uint8ClampedArray}` and assert different `TypeId`s.
This is the `canonicalizes_union_member_order` sibling and it proves the exact
thing the variant exists for: that no `Dict` spelling and no other view can stand
in. (Enforcement-rule note: `Type::Host` does not add `SmeltUnknown` anywhere;
it is a *removal* mechanism. The `SmeltUnknown`-enforcement paperwork applies to
the prelude wrappers in Stage 3, which keep their `Vec<SmeltUnknown>` argument
vector inside `RuntimePrelude`.)

### Stage 2 — produce the variant, with a total erase-on-use fallback

- `byte_buffer_constructor_expression` (`new_expr.rs:1131`) interns
  `Type::Host { class }` instead of `Type::Unknown` (`:1138`, `:1150`);
  `buffer_constructor_expression` (`lowering/stdlib/buffer.rs:113`) likewise.
- `emitter/types.rs:761` emits `SmeltRecord<String, SmeltUnknown>` for `Host`.
- `host_construct_text` (`emitter/host_interop.rs:77`) keeps calling
  `smelt_reflected_construct` but now wraps its result with the new zero-copy
  `SmeltRecord::from_unknown_object` (§4.2).
- **The fallback, which is the whole point of this stage.** One general rule: any
  member read, member call, index read, index write, `instanceof`, `typeof`,
  `Object.keys`, or string coercion on a `Host`-typed receiver for which no typed
  rule exists **erases the receiver through the aliasing adapter and reuses the
  existing `Unknown` lowering verbatim**. Implemented once, at the point where the
  frontend gives up on a receiver — not per operation.
- The 240 `Some(Type::Unknown` gates (§1.5) are triaged in this stage's diff:
  each either gains `Type::Host` (when the erased path is still the right answer)
  or is left alone (when the fallback covers it).

**Checkpoint 2.** All three corpora at **≥ current** (es-toolkit ≥ 875/184,
remeda 1789/0, radash the same 3 errors). Avoidable erasure: expect
**≈ 35,738 ± 30** — flat, possibly slightly worse, because the constructor door
has not changed yet and the fallback adds an adapter call per erased use. **A
regression here of more than ~50 means the fallback is not total and the stage is
wrong.** Do not proceed to Stage 3 to "fix" a Stage-2 corpus regression.

**Regression test** (`crates/smelt-codegen-rust/src/tests/part_7_tests.rs`, where
the typed-array goldens already live): `host_typed_value_falls_back_by_erasing` —
a golden asserting that an unmodelled member read on a host-typed receiver emits
the aliasing erase followed by the *same* dynamic-field-read text an
`Unknown`-typed receiver emits. Plus
`host_record_erase_round_trip_shares_backing` in
`crates/smelt-codegen-rust/tests/typed_array_runtime.rs` (`#[ignore]`d by
convention): erase a view, write an element through the erased alias, read it back
through the typed record, assert the write is visible. This is the test that
catches the copying `IntoSmeltUnknown`/`SmeltFromUnknown` trap of §4.2 — the one
failure a golden cannot see.

### Stage 3 — typed constructor doors (**this is where the metric moves**)

- The six new prelude entries of §4.1, each a wrapper over the existing
  `smelt_host_buffer_construct` body.
- Form selection in the frontend from the argument's HIR type — a `match` over the
  lattice, no class names. `Unknown`-typed arguments keep the
  `smelt_reflected_construct` door.
- `smelt_host_record_index_assign` for indexed writes on a host-typed receiver.

**Checkpoint 3.** Corpora ≥ Checkpoint 2. Avoidable erasure: the 137 non-prelude
construct sites carry **179** tokens today, of which **150** are argument
erasure. Expect **−100 to −150**, landing at roughly **35,590–35,640** — under the
35,677 ratchet. If it lands above 35,677, the argument selection is not firing at
some sites; find them with
`smelt smelt-unknown-report … --baseline blocker-logs/smelt-unknown-baseline-es-toolkit.json`
and read the surviving shapes before touching anything else.

**Regression test** (`part_7_tests.rs`, string goldens, CI-run):
`constructor_form_is_selected_from_argument_type` — four goldens over one class
proving `new X()`, `new X(8)`, `new X([1,2,3])`, and `new X(buf, 1, 2)` each emit
their own typed door and that **none** of the four emits
`smelt_reflected_construct`; plus `reflected_construct_survives_for_dynamic_class`
proving an `Unknown`-typed callee still emits it. Plus, in
`typed_array_runtime.rs`, `typed_and_reflected_records_stay_structurally_equal` —
build one view through the typed door and one through
`smelt_reflected_construct` and assert `==`, which is the invariant
`reflection_prelude.rs:1-37` exists to protect and the one §4.1 is most at risk
of breaking.

### Stage 4 — the dispatch fix

- `TypeScriptReceiverKind` gains `ByteStorage` / `TypedArrayView` /
  `ByteAddressedView`; `TYPESCRIPT_METHODS` gains capability-keyed rows for the
  first-cut member set of §3.3.
- `Self::dispatch_host_object_method`, inserted before
  `Self::dispatch_collection_method` at `call_dispatch.rs:1142`.
- `Uint8Array.prototype.set` lowered to the element byte-copy op.
- `instanceof` static folding for host-typed operands (§2.3).

**Checkpoint 4 — the phase exit.** `Uint8Array.prototype.set` dispatches
correctly and `destView.set(srcView)` copies bytes (assert with a throwaway
`src/predicate/zzprobe.spec.ts` per `estk-typed-array-views.md` "Reproducing").
Corpora **>** current is expected here, not merely ≥: the `clone.ts:99` DataView
clone currently no-ops, so `clone`/`cloneDeep` DataView specs should flip. Also
expect a further **−10 to −25** avoidable from the `instanceof` folding and from
the giant dynamic-`"set"`-read blob at `clone_1.rs:219` disappearing. Target
**≤ 35,600**.

**Regression test.** In `crates/smelt-stdlib/src/recognition.rs` `mod tests`
(where the `(receiver, member, rule)` table is already asserted):
`map_set_and_typed_array_set_are_different_rules` — assert
`typescript_method_rule(Map, "set") == Some(TsMapMutation)` **and**
`typescript_method_rule(TypedArrayView, "set") != Some(TsMapMutation)`, i.e. the
collision cannot be reintroduced by adding a row. In the frontend tests, rename
the pair that already changed shape in the previous pass and add
`typed_array_set_does_not_dispatch_as_map_set` — lower
`new Uint8Array(b).set(new Uint8Array(c))` and assert the HIR is the byte-copy op
and **not** `ExprKind::DictSet`, with the *negative* assertion being the point:
this is the test that fails loudly if anyone ever routes `Host` through the `Dict`
arm at `collections.rs:71` again. In `typed_array_runtime.rs`:
`prototype_set_copies_bytes_between_views` and, mirroring the source shape,
`data_view_clone_copies_its_bytes`.

### Stage 5 — re-snapshot and record

- Regenerate the es-toolkit report; if avoidable **fell**, re-snapshot
  `blocker-logs/smelt-unknown-baseline-es-toolkit.json` **in the same commit**
  (per `AGENTS.md`).
- Verify `blocker-logs/smelt-unknown-baseline.json` (examples corpus) still has
  avoidable **== 0** — the hard invariant.
- Write the delta table into `blocker-logs/estk-typed-array-views.md` closing the
  "needs a maintainer decision" section: the resolution was option 3, done
  properly.

---

## 6 · Risks and honest unknowns

Marked **[needs compile]** or **[needs corpus run]** where I could not settle it
by reading.

**R1 — The 240 erased-receiver gates are the real work, and I cannot size them.**
**[needs compile + corpus run]** §1.5 counts them; I read a sample, not all 240.
Each is a place where "receiver is `Unknown`" meant "probe the marker". Some need
`Type::Host` added, some are covered by the Stage-2 fallback, and I cannot tell
which without compiling and diffing the corpus. *Experiment that settles it:* land
Stage 2 with the fallback, diff the generated es-toolkit crate against current, and
read every changed line. Every diff at Stage 2 is either a gate that needed `Host`
or a fallback hole. This is why Stage 2's checkpoint is "corpora flat", not
"corpora better" — it is a pure-refactor gate whose only job is to prove the
fallback is total.

**R2 — My avoidable-erasure numbers are approximate.** **[needs corpus run]** I
reimplemented `classify_line` in Python over
`target/compat-repos/es-toolkit/dist-smelt/src` and got avoidable **35,842**
against the reported **35,738** (+104, 0.3%). The gap is probably the scanner's
prelude fallback (`update_prelude_helper_state`) for files without the
`@smelt:prelude-end` marker, and possibly a stale corpus in `target/`. All my
*deltas* (179, 150, 137, 12 files) come from the same consistent measurement and
are reliable as relative figures; the absolutes are not. Settle by running
`smelt smelt-unknown-report` for real.

**R3 — `type_contains_unknown` returning `false` for `Host` may cascade.**
**[needs compile]** `emitter/core.rs:3446` feeds decisions about whether a type
needs erasure treatment. Making `Host` "not unknown" is correct (that is the whole
claim) but it changes the answer for every composite containing one:
`Host[]`, `Record<string, Host>`, `Host | null`. Combined with
`union_member_is_concrete` returning `true` (`union.rs:180`), a union like
`Uint8Array | null` starts generating a **new concrete Rust enum** where it
previously erased. That is the desired direction and probably a further erasure
win — but new generated enums are new compile surface, and radash already has 3
compile errors. *Experiment:* after Stage 1, grep the generated crates for new
`enum Smelt…` definitions and check the count is what you intended.

**R4 — I do not know whether `Buffer`-assignable-to-`Uint8Array` is load-bearing
in-corpus.** **[needs corpus run]** I specified the assignability arm from the
spec relation (`to_string_tag`), not from a measured failure. es-toolkit's
`isEqualWith should compare buffers` already fails for unrelated documented
reasons (`isBuffer` is a constant `false` — `estk-typed-array-views.md` "The one
regression"), so I could not use it as a probe. If the arm turns out unneeded,
keep it anyway: without it the code silently answers `false` at
`assignability.rs:151`, which is worse than a redundant rule.

**R5 — `SmeltRecord::eq` has no cycle guard where `SmeltObject`'s does.**
Detailed in §4.3. Safe today because byte-buffer records are acyclic. It becomes a
stack overflow the moment a host record gains a back-reference. Stage 4's
regression test pins acyclicity; it does not fix the guard. Flagging rather than
fixing, because fixing it means touching `lib.rs:875` which another agent is
editing.

**R6 — Deep-nested `||` short-circuit lowering already overflows the emitter
stack.** `estk-typed-array-views.md` §5 records that a twelve-deep left-nested
`||` chain overflowed, and that `ArrayBuffer.isView` had to be built as a
*balanced* tree. §2.3's `instanceof` static folding *removes* disjunctions rather
than adding them, so Phase 2 should improve this — but if any new registry-derived
disjunction is emitted, it must be balanced the same way. Pre-existing scalability
bug, not Phase 2's, but easy to trip.

**R7 — `emitter/map.rs`'s 13 `Dict | JsMap` destructures are the collision's
mirror image.** They currently *accept* anything shaped like a dict. They will
correctly decline a `Host` receiver — but I verified this by reading the patterns,
not by compiling. If any of them reaches a host receiver through a path I did not
trace, the symptom is a wrong-shape emission rather than a compile error.
**[needs compile]** *Experiment:* temporarily make the `Host` arm of
`type_text_with_scoped_type_params` emit a distinct newtype instead of
`SmeltRecord<String, SmeltUnknown>`; anything in `map.rs` that wrongly admits a
host receiver then fails to type-check and names itself. Revert the newtype after.

**R8 — Files under concurrent edit.** `emitter/coercion.rs` is not among the
files the other agents named, but `crates/smelt-codegen-rust/src/lib.rs`
(`SmeltRecord`, `SmeltObject`, `IntoSmeltUnknown`, `SmeltFromUnknown` — every
prelude fact §4.2 depends on), `byte_buffer_prelude.rs` (the constructor body §4.1
wraps), `reflection_prelude.rs` (`smelt_reflected_construct`), and
`lowering/new_expr.rs` + `lowering/stdlib/**` (every frontend line number in §2
and §3) all are. Re-resolve `lib.rs:659` (`SmeltRecord`), `:1081` (`SmeltObject`),
`:1091` (`from_unknown_record`), `:2838` (`SmeltFromUnknown`), `:2976`
(`IntoSmeltUnknown`), `byte_buffer_prelude.rs:480` (`CONSTRUCT`),
`reflection_prelude.rs:217`, `new_expr.rs:1131`, and
`collections.rs:70-76`/`:436` **by name** before editing. Phase 1 (finding A) is
moving the prelude's source of truth into `smelt-runtime`; if Phase 1 lands first,
§4.1's six wrappers and §4.2's two adapters should be written as **real compiled
Rust in `smelt-runtime`** with unit tests, which is strictly better than the
`writer.line(...)` strings this spec describes.

**R9 — What I did not verify at all.** That the six first-cut members of §3.3 are
sufficient for the corpora. My survey was a regex over identifier names
(`view|buffer|array|destView|…` `.member`), which under-counts members reached
through aliases or destructuring and over-counts `.length`/`.slice` on ordinary
arrays. **[needs corpus run]** The Stage-2 fallback is the mitigation: any member
I missed keeps working through the erased path, at the cost of a few avoidable
tokens — which is exactly the trade Stage 2 is designed to make safe.
