# Record identity: one object behind every view

Status: in progress. Section E of the round-2 audit ("identity and by-value copies"). Base: `origin/main` at
`7035aeb56`. Builds on `docs/design/class-prototypes.md`, #503 (structural views), #512 (object literals are one
object at every record slot) and #524 (views at a wider property type, and at two record types).

## 0. The problem

A JavaScript object is a reference. Every name, parameter, array slot and `unknown` that holds it holds THE
object: a write through one is read through all of them, and `===` compares the reference. A Rust port that
converts a value to another static type by building a new value of that type (a field-wise copy) breaks both:

```ts
class Pt { constructor(public x: number, public label: string) {} }
interface Labeled { label: string }
function rename(l: Labeled) { l.label = "renamed"; }
const p = new Pt(1, "a");
rename(p); p.label;                          // Node "renamed"; Smelt at the base: "a"
const kept: Labeled = p; p.label = "z";
kept.label; kept === p;                      // Node "z" true; Smelt at the base: "a" false
```

What is left of this after #503/#512/#524, measured with one probe per item (Node vs generated Rust, base binary):

| item | probe | base |
|---|---|---|
| class instance at an interface slot | `rename(p)` above | copy: write lost, `===` false |
| typed list at an `unknown[]` slot | `fill(nums as unknown[])` writes `xs[0]`, `push` | copy: writes lost |
| namespace object at a record slot | `const v: Shape = NS` (`import * as NS`) | not modelled: `NaN`, `[object Object]` |
| host object at a record slot | `const v: HasLast = /a/g` | E0308 |
| class with accessors at a record slot | `const v: HasV = objWithGetSetV` | E0308 |
| function at an object-type slot | `const n: Named = hello` (`name`, `length`) | the type's default (`""`, `0`) |

## 1. What a hand-written port does

A team porting by hand keeps a plain struct wherever nothing observes identity, and reaches for a shared handle
only where something does. For an object seen through several static types it writes what Smelt already writes
for records (`crate::structural_views`) and class hierarchies (`crate::class_polymorphism`):

```rust
struct Pt(Rc<RefCell<PtInner>>);                 // the object, as built
struct Labeled(Rc<dyn LabeledSmeltView>);        // any object used as a Labeled
trait LabeledSmeltView {                         // one accessor pair per declared property
    fn label(&self) -> Ref<'_, JsString>;
    fn label_mut(&self) -> RefMut<'_, JsString>;
    fn smelt_any(self: Rc<Self>) -> Rc<dyn Any>; // checked cast back
    fn into_smelt_unknown_as_concrete(self: Rc<Self>) -> SmeltUnknown;
}
impl LabeledSmeltView for RefCell<PtInner> { /* lends &inner.label */ }
```

A conversion is an unsizing coercion of the same `Rc` (`Labeled(p.0.clone())`): one allocation, so identity
(the cell address), writes in both directions, and the erased form (the concrete record's own erasure) all hold.

## 2. Which values need a shared handle

The rule is the one `classify::reference_classes` already applies, extended to every new participant:

* **Plain value** (by-value struct, `Copy`/`Clone` of the fields): a record that is never mutated after
  construction, never compared by identity, never converted to another record type, never erased into a dynamic
  slot. Copying such a value is observationally identical to sharing it, so it stays a plain struct. This is the
  common case and keeps its exact previous output.
* **Reference record** (`Rc<RefCell<Inner>>`): one of the classification triggers holds (mutation, identity
  comparison, `this` escaping, erasure into a dynamic slot, structural view participation, ...).
* **View target** (`Rc<dyn TargetSmeltView>`): a record type some OTHER record type converts to. Only these pay
  dynamic dispatch on property access; their sources keep direct field access.
* **Polymorphic class** (`Rc<dyn BaseSmeltCell>`): a reference class with subclasses.

Erased values (`SmeltUnknown::Object`, `SmeltUnknown::Array`) are already handles; an erasure of a reference
record is a live view keyed by the cell address (`smelt_reference_object_identity`), so `erase(a) === erase(a)`.

## 3. Conversions share storage

| from \ to | record type `T` | `unknown` / erased |
|---|---|---|
| record `S` sharing `T`'s properties | view: `T(s.0.clone())` (#503) | live erased view (#503) |
| record `S` at a wider property type | converting view guards (#524) | as above |
| **class instance** (this design, step 1) | view: `T(c.0.clone())`, the class cell implements `T`'s trait | the class's own erasure, with its class tag |
| typed list `E[]` at `unknown[]` (step 2) | — | a live list view (see §5) |
| erased value at `T` | checked cast: the origin cell when it is one, else the field-wise rebuild | — |

A cast back (`kept as Pt`) is checked through `smelt_any`: the `Rc` of the cell the object was built as, when it
is one of the target's cells. A whole-object operation (keys, JSON, inspection, deep equality, `instanceof`) asks
the object, never the view: the trait's erasure method hands out the concrete record's own erasure.

## 4. Cost model

* A program with no conversion between distinct record types emits exactly what it emitted before: no view
  trait, no `dyn`, no extra field.
* A source type keeps its concrete handle and direct field access (`p.0.borrow().x`). Only the view TARGET's
  property accesses go through a vtable, and only when some other type is converted to it.
* A conversion is one refcount increment. There is no per-conversion allocation, and no per-access conversion
  unless the target declares the property wider than the source stores it (#524's guards).
* Nothing is erased by a conversion. `SmeltUnknown` appears only at the typed-to-erased crossing the record
  already had (its `IntoSmeltUnknown` impl), which the SmeltUnknown report classifies as a boundary.

## 5. Steps

1. **Class instances as view sources.** A non-generic class whose handle is `Rc<RefCell<NameInner>>`
   (`structural_views::is_viewable_class`: no subclass, not a statics frame, no index store, no call signature)
   lends its own stored data fields to a view (`view_source_fields`): not methods (prototype), not virtual
   method slots, not `get`/`set` accessors, not hidden fields. A target declaring one of those keeps the
   field-wise conversion. A subclass qualifies (its cell dereferences to its base's fields), so a view over a
   `Dog` is a view over the one `Dog` object.
   * The generated `IntoSmeltUnknown` of a reference class now stamps the `__smelt_class` tag, as the inline
     erasure always did: a class erased through a view (or any generic path) answers `instanceof` and
     `constructor` as the class it was built as.
2. **Typed lists at `unknown[]`.** `SmeltList<SmeltUnknown>` built from a typed `SmeltList<E>` shares the typed
   list through the existing live-array protocol (`SmeltLiveList`, used today by `SmeltUnknown::Array` views of
   typed lists): reads convert each element out, writes convert each element in (a value the element type
   cannot hold is converted to it, as for a write through a widened view). No new erasure: the slot is
   `unknown[]` in the source.
3. **Accessor-backed and host properties.** A view accessor answered by a getter/setter pair (a class `get v()`,
   `RegExp.lastIndex`) instead of a lent field, with the converting guards #524 introduced.
4. **Namespace objects.** A module namespace as a value is a generated struct whose accessors read the module's
   live bindings; a view of it implements the target's trait the same way; its write accessor throws
   (`TypeError`, ES module namespaces are read-only).

Each step ships as its own PR with a Node-parity fixture, its stated requirements and a planted defect the
fixture detects.

## 6. Requirements

Behavior, for a value `s` of source type `S` and its view `t` at type `T`:

* a write through `t` is read back from `s`, and a write to `s` is read back through `t`, however long `t` is
  held;
* `t === s`, `(t as S) === s`, and two views of `s` compare equal;
* a whole-object read of `t` (keys, JSON, `console.log`, `instanceof`) answers what it answers for `s`.

Type preservation: `S` keeps its struct and field types; `T`'s accessors are typed by `T`'s declared property
types; no value is erased by a conversion; the SmeltUnknown and typedness ratchets do not rise.

## 7. Known gaps

* A class WITH subclasses (polymorphic, `Rc<dyn BaseSmeltCell>`) viewed as an interface still copies: its cell
  trait would need a `smelt_as_<Target>` method implemented by every concrete cell of the hierarchy.
* Generic classes and generic shapes keep the field-wise conversion (one cell type per instantiation).
