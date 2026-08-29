# Struct-shaped objects — what landed, what did not, and the staged plan

Goal: an object whose shape is statically known should lower to a **generated Rust
struct with typed fields**, not to a string-keyed hash map of erased values —
"types correct all the way down to runtime", applied to objects.

Baseline profile that motivated it (`partition`, 10 000 records, callgrind):
~43 % malloc/free/drop, 16.2 % SipHash, 6.2 % `SmeltObject::insert`, and 2.7 %
in the transpiled function itself. The cost is the runtime representation, not
the code.

---

## 1. What Smelt already had

There is **no record type in HIR**. `crates/smelt-hir/src/ty.rs` has no
`Type::Record`; an inline object type literal `{ id: number; group: string }`
lowers in `lowering/ty/annotations.rs::type_literal_to_hir` to
`Dict(String, <union of the field types>)`, and the field names are gone from the
type before MIR ever sees it.

What *does* exist, and is the right vehicle:

* an `interface` lowers to `smelt_hir::Item::Interface` with named `Field`s, a
  `Type::Class { name }` reference, and a **generated Rust struct** with one
  typed field per member (`crates/smelt-codegen-rust/src/lib.rs`, the
  `mir.interfaces` emission loop);
* an object literal flowing into an interface-typed position is already adapted
  field-wise (`emitter/core.rs::structural_record_from_string_dict_adapter_text`);
* `decls/callable_object.rs` already **synthesizes an anonymous interface** from
  a structural type literal, named by a hash of its members so identical shapes
  share one generated struct. It only fires for *callable* object surfaces.

So the shape feature is not "invent a representation", it is "route pure-data
object type literals through the synthesis `callable_object` already performs" —
plus everything the resulting structs need in order to behave like JavaScript
objects.

## 2. What landed (green)

The *machinery* a shape struct needs, all of which is independently a bug fix for
the `interface` spelling that already reaches these paths:

1. **Shapes participate in reference-class lifting.**
   `classify::class_name_of_local` now matches `mir.interfaces` as well as
   `mir.classes`, and a lifted shape is emitted through the handle newtype
   (`emit_reference_record_storage`, factored out of the old
   `emit_reference_class_storage`). Before this, `interface Flags { era?: number }`
   with `flags.era = value` was emitted as a by-value struct and the write was
   invisible through every alias — the aliasing bug the task calls out, already
   present for the `interface` spelling. `reference_class_tests` carried a test
   asserting that gap; it is now inverted.
2. **Reference records have a stable erased object identity.**
   `smelt_reference_object_identity(cell_address)` maps one `Rc` cell to one
   erased object id, so erasing two handles on the same object yields values that
   compare `===` equal. Previously every erasure minted a fresh id.
3. **Interface-backed records derive `PartialEq`** under the same rule value
   classes use (every stored field comparable), and `type_supports_partial_eq`
   now recurses through interfaces so a record holding a callback-carrying shape
   does not derive one either.
4. **An interface-backed record no longer stamps `__smelt_class` when erased.**
   The inline class-erase adapter in `emitter/coercion.rs` stamped a class marker
   on interface records while the interface's own `IntoSmeltUnknown` did not, so
   the same value erased two different ways depending on the path taken. A
   structural record is a plain object in JavaScript: no constructor, no marker.
5. **Union injection breaks ties by field compatibility.** `{ a: number } | { b: string }`
   left several members shape-compatible, which the "unique shape match" rule
   rejected outright; the narrowing keeps only members whose required fields the
   source can populate.
6. **New runtime tier** `crates/smelt-codegen-rust/tests/object_shape_reference_semantics_runtime.rs`
   — the object twin of `list_reference_semantics_runtime`: it lowers TypeScript
   fixtures to crates and *runs* them. Four cases pass; two are **inverted
   characterization tests** pinning gaps that are real today (see §4).

## 3. What did not land: the frontend hook

The one-line change that turns the feature on is in
`lowering/ty/annotations.rs::type_literal_to_hir`:

```rust
if let Some(shape_ty) = self.anonymous_record_shape_type(literal) {
    return Ok(shape_ty);
}
```

backed by `lowering/decls/shape_object.rs` (written, documented, and working —
see the WIP commit named in the hand-off). It qualifies a type literal when every
member is a plain named property signature with an annotation, and rejects index
signatures, call/construct signatures, method signatures, unresolvable computed
keys, the empty literal, type-parameter-mentioning fields, and **function-typed
fields** (`{ valueOf: () => number }` is a duck-typing surface, not a record — a
`Date` satisfies it without being one, so a nominal struct would make the union
`number | { valueOf(): number }` claim to be closed when it is not).

With the hook on, the emitted code is exactly what the task asked for — e.g.
`__param2.leading` becomes a struct field load instead of
`map.get("leading")` — and 53 distinct shapes are generated across remeda, 21
across es-toolkit. It is **not green**, for two reasons, both of which are the
same missing capability:

### 3a. A by-value record has no JavaScript object identity

es-toolkit's `memoize` spec instantiates `ImmutableCache<T>` at a shape type. The
cache requires `T: SmeltFromUnknown + SmeltJsKeyEq`; `SmeltRecord<String, V>`
satisfies both (it carries an `id`), a generated struct satisfies neither.
`SmeltJsKeyEq` cannot be implemented honestly for a by-value struct, because
JavaScript keys objects by *reference* and a by-value struct has no reference to
key on. **Six compile errors, so the whole es-toolkit crate fails to build.**

### 3b. An erased-and-mutated record cannot share storage

remeda `constant` — "returns identity (doesn't clone)":

```ts
const obj = {} as { a?: boolean };
const firstInvocation = constant(obj)();   // obj erased to SmeltUnknown here
obj.a = true;
expect(firstInvocation).toStrictEqual({ a: true });   // fails
```

Erasing a typed record to `SmeltUnknown::Object` **copies** its entries, so a
later mutation is invisible through the erased alias. `crates/smelt-mir/src/erased_record_promote.rs`
solves this for dict-backed records by demoting the value type to `Unknown` so
the erasure can share the `Rc<RefCell<..>>` via `SmeltObject::from_unknown_record`.
A struct has no such move: its layout cannot back a `HashMap<String, SmeltUnknown>`.
The identity half now holds (§2.2 — `expect(firstInvocation).toBe(obj)` passes);
the *contents* half does not. **One remeda failure: 1788 passed / 1 failed.**

Measured with the hook on: remeda 1788/1, radash 84/0, es-toolkit **does not
compile**.

## 4. The two inverted runtime tests

`known_gap_a_local_alias_does_not_yet_share_the_object` and
`known_gap_a_read_back_handle_from_an_array_does_not_yet_share` assert that the
generated crate still FAILS. Both are true of *today's* dict lowering, before any
shape work:

```ts
const a: { count: number } = { count: 1 };
const b = a;
b.count = 2;
expect(a.count).toBe(2);   // fails today
```

A record local whose fields are written is lifted to a shared cell at its
*parameter* sites (which is why `a_callee_mutates_the_callers_object` passes) but
not at its binding sites. When that closes, the tests go red and should be
flipped to `run_object_fixture`.

## 5. Staged plan for the rest

**Stage 1 — give generated record structs a JavaScript object identity.**
Everything else is blocked on this. Add a hidden identity slot to every generated
record struct (classes and shapes), minted by `smelt_next_object_id()` at
construction and carried through `Default`, the field-wise adapters, the union
arms, and both erasure paths. Then `SmeltJsKeyEq` is `self.id == other.id` (JS
reference keying), `SmeltFromUnknown` reconstructs with `with_id`, and `===`
between two records is honest. This is what unblocks §3a. Expect churn in every
struct-literal emission site; do it on its own branch with the corpora as the
gate.

**Stage 2 — representation choice for erased-and-mutated shapes.**
The emitter already knows, per shape symbol, both facts it needs: `classify::reference_classes`
says "mutated", and the erase paths say "crosses a dynamic boundary". A shape
that is both cannot be a struct — the same conclusion `erased_record_promote`
reaches for dicts. Make it a *codegen-level* representation choice: such a shape
emits as `SmeltRecord<String, SmeltUnknown>`, and `place.rs` emits keyed
get/set for its `Place::Field` instead of `.field`. This must live in the emitter,
not in MIR: demoting `Class{S}` to `Dict` in MIR is not a type-only rewrite,
because a property read on a dict-typed value is a different MIR shape
(`DictGet`) than `Place::Field`. This unblocks §3b.

**Stage 3 — turn the hook on**, re-run the corpora, and flip the two inverted
runtime tests.

**Stage 4 — cheaper construction.** An object literal flowing into a shape
currently builds an intermediate `SmeltRecord` and reads it back
(`{ let smelt_record_map = …; Shape { a: smelt_record_map.get("a")… } }`). A
`DictLit` whose keys statically match the target shape should construct the
struct directly. This is where the malloc reduction the profile asks for actually
comes from; stages 1–3 only make the representation available.

**Stage 5 — inferred object literals.** Only the *annotation* position is wired
(`type_literal_to_hir`). `const x = { a: 1, b: "s" }` with no annotation still
infers a `Dict`. Extending `object_literal_type` to synthesize the same shape is
the second half of the win, and should follow the same qualification rule so the
two spellings agree.

## 6. Note on the benchmark

The es-toolkit `partition` / `group_by` / `count_by` / `uniq_by` benchmarks build
their records **in the hand-written harness** as `SmeltUnknown::Object(SmeltObject::new(..))`
and call `entry_partition::<SmeltUnknown, _>`. The transpiled function is
monomorphized at `T = SmeltUnknown`, so **no shape struct is on that path** and
struct-shaped objects cannot move those numbers as the benchmark is written
today. Getting the profile to reflect the feature needs the harness to pass
*typed* records — i.e. the benchmark cases have to be re-expressed in TypeScript
against a declared shape rather than assembled as erased values in Rust. That is
worth doing before the next round of measurement, or stages 1–4 will land with no
number to show for them.

Separately: `benchmarks/rust/smelt_bench_cases_es_toolkit.rs` did not compile on
this branch — `SmeltList::iter()` yields owned values since the list-ABI change
(61f9171) while the checksum folds still passed them by reference. Repaired here
(three call sites) so `benchmarks/prepare.py` works again.
