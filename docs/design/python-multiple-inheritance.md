# Python multiple inheritance: several bases, one object

Status: implemented. Replaces the refusal "class 'C': multiple inheritance is not supported". Code:
`crates/smelt-hir/src/mro.rs` (C3 linearization), `crates/smelt-frontend-py/src/lowering/class.rs` (bases,
method resolution, `super()`), `crates/smelt-codegen-rust/src/class_polymorphism.rs` (storage),
`crates/smelt-codegen-rust/src/emitter/structural_view.rs` (field access).

## 0. The problem

```python
class Named:
    def __init__(self, name: str) -> None:
        super().__init__()
        self.name = name
    def label(self) -> str:
        return "named " + self.name

class Counted:
    def __init__(self) -> None:
        super().__init__()
        self.count = 0
    def bump(self) -> int:
        self.count += 1
        return self.count

class Item(Named, Counted):
    def __init__(self, name: str) -> None:
        super().__init__(name)          # Named.__init__, whose super() is Counted.__init__

def show(n: Named) -> str: return n.label()
def tick(c: Counted) -> int: return c.bump()

it = Item("x")
tick(it); tick(it)
print(show(it), it.count)               # named x 2
```

An `Item` is one object that is a `Named` and a `Counted` at once: passed as either, a write through one view
is seen by the other, and `is` compares the same object. Cooperative `super()` follows the *instance's* method
resolution order (MRO), so `super()` inside `Named.__init__` reaches `Counted.__init__` when the instance is an
`Item`, and `object.__init__` when it is a plain `Named`. A diamond (`D(B, C)`, `B(A)`, `C(A)`) stores `A`'s
attributes once and runs `A.__init__` once.

Smelt's single-inheritance storage cannot hold this: a polymorphic class's handle is `Rc<dyn BaseSmeltCell>`
whose `borrow()` lends the base's inner record, and a subclass nests that record as `__smelt_base` and `Deref`s
to it (`class_polymorphism.rs`). A class with two bases would need two nested records, and a diamond would hold
the shared base twice, so a `Counted` view of an `Item` cannot be a borrow of one contiguous `CountedInner`.

## 1. What a hand-written port does

A Rust team porting a mixin hierarchy writes one struct per concrete class holding every attribute the instance
has, and one trait per class used as a base, with an accessor pair per attribute; each concrete struct
implements the trait of every class it derives from. That is the representation, restricted to the classes
that need it.

## 2. Data model

**HIR.** `Class.base` stays the first program base; `Class.extra_bases` lists the others in declaration order
(empty for every TypeScript class and every single-inheritance Python class, so no existing consumer changes
meaning). `smelt_hir::mro::linearize` computes CPython's C3 linearization from a class's bases; the frontend
refuses a class whose bases have no consistent linearization (CPython raises `TypeError` at class creation, so
no valid program has one).

**Attributes are the instance's, by name.** A Python instance has one attribute namespace: `self.x` assigned by
`A.__init__` and by `B.__init__` is one slot. A class's fields are the union, by name, of the fields of every
class in its MRO, collected from the root of the MRO towards the class (the order single inheritance already
used: base fields first). A field declared by two classes of one MRO at different types is refused.

**Methods resolve along the MRO.** The method a class runs for a name is the first class in its MRO defining it.
Smelt already emits each inherited method body on every class that inherits it (`self` is the inheriting
class); with several bases the inherited set is collected over the MRO instead of the base chain.

## 3. Storage

A *multiple-inheritance hierarchy* is a set of classes connected by base edges (any base, in either direction)
of which at least one class has more than one program base. Every other hierarchy keeps its storage
byte-for-byte. Inside an MI hierarchy:

- **Storage is flat.** No inner record nests another: `ItemInner` holds `name` and `count` directly. A diamond
  holds `A`'s fields once because they are one name each.
- **Every class of the hierarchy is a reference class** (`classify.rs`): an object viewed as each of its bases must
  be one shared cell.
- **A class with subclasses is polymorphic**, as before, but its cell trait lends a *field view* instead of its
  own inner record. Each field's accessor pair is declared by exactly one view trait, the one of the furthest
  class of the MRO that has the field (`field_view_owner`); a view extends the views of its ancestors. A record
  implementing several views (a diamond's bottom implements `Base`, `Left` and `Right`) therefore has one
  accessor per field, never two candidates:

```text
struct Named(Rc<dyn NamedSmeltCell>);            // any instance of Named or below
trait NamedSmeltFields: Debug {                  // the fields Named is the furthest owner of
    fn name(&self) -> &String;
    fn name_mut(&mut self) -> &mut String;
}
trait NamedSmeltCell {
    fn borrow(&self) -> Ref<'_, dyn NamedSmeltFields>;
    fn borrow_mut(&self) -> RefMut<'_, dyn NamedSmeltFields>;
    fn try_borrow(&self) -> Result<Ref<'_, dyn NamedSmeltFields>, BorrowError>;
    fn smelt_any(self: Rc<Self>) -> Rc<dyn Any>;
    fn smelt_class_name(&self) -> &'static str;
    fn smelt_as_<Ancestor>(self: Rc<Self>) -> Rc<dyn <Ancestor>SmeltCell>;   // every polymorphic strict ancestor
}
impl NamedSmeltFields for NamedInner { .. }      // and for ItemInner: `&self.name`
impl NamedSmeltCell for RefCell<NamedInner> { .. }
impl NamedSmeltCell for RefCell<ItemInner> { .. }
```

- **A field access through a polymorphic MI handle calls the owning view's accessor, fully qualified**:
  `(*NamedSmeltFields::name(&*n.0.borrow()))` for a read, `(*NamedSmeltFields::name_mut(&mut *n.0.borrow_mut()))`
  for a write. The qualified call also resolves on a concrete record: an inherited body emitted on a subclass
  (`self` typed as the base in HIR) reads the subclass's own flat record, which implements every view of its MRO.
  A concrete leaf handle keeps `.0.borrow().name`. The erasure and recovery impls read and write through the same
  accessors (`guard_field_place`).
- **An upcast is the same `Rc`, unsized** (`Counted(item.0.clone())`), or re-viewed through `smelt_as_<Target>`
  from a polymorphic source, toward *any* ancestor in the MRO, not only along the first-base chain.
- **A downcast and `isinstance` are checked** through `smelt_any` over the target's concrete classes, which are
  the classes whose MRO contains the target.
- **Override dispatch** is unchanged in shape: a polymorphic class's method downcasts to each concrete class
  below it (now: whose MRO contains it) that resolves the method to another body, and forwards.

## 4. `super()` and `__init__`

`super().f(...)` in a body of class `K`, run on an instance of class `C`, calls the `f` of the first class after
`K` in `C`'s MRO that defines it. Smelt already gives every class `C` one alias per (definer `K`, method `f`)
(`reserve_super_call_aliases`); the alias's target is now looked up in `C`'s MRO instead of `K`'s base chain,
which is the same thing under single inheritance.

`super().__init__(...)` follows the same rule. Under single inheritance Smelt constructs a separate base object
and copies its fields back; with several bases that runs the wrong chain (`Named.__init__`'s `super()` would
reach `object` instead of `Counted`), so inside an MI hierarchy `__init__` bodies are also methods: each class
`C` gets, per definer `K` of its MRO, an alias running the `__init__` of the class after `K` in `C`'s MRO on
`self`; a constructor allocates the instance with every field at its default and runs the class's own
`__init__` body, whose `super().__init__(...)` calls that alias. When no class after `K` defines `__init__`, the
call is `object.__init__()` and does nothing. An explicit `Base.__init__(self, ...)` (or `Base.f(self, ...)`) on
a class in the MRO runs that class's body on `self` the same way.

## 5. Requirements

Behavior (checked against `python3` by `fixtures/python-parity/multiple_inheritance.py`):
- method resolution and `super()` chains follow C3, including a diamond and a mixin ordered before a base;
- `__init__` of a diamond's shared base runs once, cooperative chains run in MRO order;
- an object passed as each of its bases is the same object: writes through one base view are read through the
  other, `is` holds, `isinstance` answers for every class in the MRO;
- a call through a base-typed handle runs the instance's resolved method.

Type preservation: every field keeps its declared type in every view (accessors are typed by the field), no
value is erased, no `SmeltUnknown` is introduced.

## 6. What stays refused

- A class in an MI hierarchy that is generic, an exception class, a dataclass, or derives from a builtin class
  (`dict`, `int`, ...) other than `object`; a base that is not a class of the same module (the hierarchy is found
  by a syntactic pre-scan of the module, `collect_multiple_inheritance_classes`, because a base is lowered before
  the subclass that makes its hierarchy one).
- Bases without a consistent C3 linearization (a `TypeError` in CPython; `ty` reports it first).

## 7. Evidence

`crates/smelt-transpiler/tests/fixtures/python-parity/multiple_inheritance.py` (its stdout equals `python3`'s).
Planted defects, each caught by that fixture and then restored:
- C3 replaced by a depth-first, left-first order: the diamond's `super()` chain skips `Right`, and `C(A, B)`
  answers `C>A>Root`.
- Definer aliases resolved along the definer's own MRO instead of the instance's (the single-inheritance rule):
  `Right.__init__` never runs on a `Bottom`, which then has `right == 0`.
- `isinstance` against a class unrelated to the static type but sharing a descendant answered statically: a
  `LoudMixin` test on an `Animal` handle holding a `Dog` prints `False`.
