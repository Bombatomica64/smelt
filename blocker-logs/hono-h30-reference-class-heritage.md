# H30 — one class, two representations

Round 11, item 2, second family: 17 of the slice's remaining errors, and two
places where a class was emitted one way and its own code was written the other
way. Both are the same mistake with different owners.

## H30a — a subclass of a reference class was emitted by value

A classification trigger (`classify::reference_classes`) fires on the class that
DECLARES the mutated field, so a subclass which only inherits mutated state is
never named by one. But Smelt re-emits every inherited method body into the
subclass's own `impl`, and those bodies were written against the base's handle
representation:

```
error[E0609]: no field `0` on type `&__smelt_anon_class_3050<T>`
 --> src/main.rs:5822
   _smelt_tmp_4 = self.0.borrow()._tries.clone()...
   = note: available fields are: `name`, `_middleware`, `_routes`, `_tries`, `match_`
```

The class is Hono's `class<T> extends RegExpRouter<T>` in
`reg-exp-router/prepared-router.ts`. `RegExpRouter` is a reference class; the
anonymous subclass was not, so it got a value struct plus inherited bodies that
project through a handle it does not have. E0609 ×9.

**Rule:** a class that inherits from a reference class IS a reference class,
closed transitively over `MirClass::base`. This is not a heuristic — a subclass's
instances are the same JavaScript objects as its base's, so identity and mutation
semantics cannot differ between them. Only DOWNWARD: a base's storage is its own
struct, and lifting a base because some subclass was lifted would pay the
handle's cost for objects that never need it. The propagation runs before the
index-store deviation, so that deviation still has the final say.

## H30b — a method VALUE of a reference class rebuilt the struct

Reading a method as a value (`return this.bump`, or Hono's
`match: typeof match<Router<T>, T> = match`) captures a receiver into a closure.
That capture re-instantiated the class field by field:

```rust
let smelt_receiver = PatternRouter::<SmeltUnknown> {
    name: self.name.clone(), _routes: self._routes.clone(), .. };
//  ^ struct `PatternRouter<SmeltUnknown>` has no field named `_routes`  (E0560)
//                          self._routes -> no field `_routes`           (E0609)
```

A reference class has exactly ONE field — the `Rc<RefCell<Inner>>` — so the
rebuild cannot be spelled. It is also the wrong meaning: a method read off an
object is bound to THAT object, and a rebuild hands the closure a copy whose
mutations nobody observes. The capture is now `self.clone()`, the handle clone
that shares the cell — the same receiver capture `class_proto` already uses for
the prototype table. The value-class path is untouched: there the rebuild is a
type conversion (it instantiates the erased `::<SmeltUnknown>` form), not a copy.

## Effect on the slice

| | errors |
| --- | ---: |
| after H25 | 79 |
| after H30a | 70 |
| after H30b | **62** |

## Tests

`crates/smelt-codegen-rust/src/tests/part_7_tests.rs`

* `a_subclass_of_a_reference_class_is_a_reference_class` — a base lifted by a
  field write and a subclass that only inherits it. It asserts the BASE is a
  handle first (so the propagation claim is not vacuous), then that the subclass
  is a handle, then that the subclass's own method reads the inherited field
  through it.
* `a_method_value_of_a_reference_class_captures_the_handle` — a reference class
  whose method is returned as a callback: the capture is `self.clone()`, and the
  field-by-field rebuild is gone.
