# Class prototypes: one object per class, with identity

Status: implemented (steps 1-3); see "Implementation notes" at the end. Base: `origin/main` at `34a277310`. Every `file:line` below refers to that tree.

Decision taken by the user: a class has a real prototype object with stable identity, its `.constructor` is the
class, and `Object.getPrototypeOf`, `Object.setPrototypeOf`, `Object.create`, `isPrototypeOf` and `instanceof`
agree with each other. Then `Object.assign` onto `unknown` writes in place, and es-toolkit's `clone` must still hold.
This document says how.

## 0. The problem, measured

A probe of 13 checks (`class Animal`, `class Dog extends Animal`, an unrelated `class Plant`, values held as
`unknown`). Node answers `true` to every check below. Smelt at the base answers:

| # | check                                                         | Smelt          |
|---|---------------------------------------------------------------|----------------|
| 1 | `getPrototypeOf(dog1) === getPrototypeOf(dog2)`               | true           |
| 2 | `getPrototypeOf(dog) !== getPrototypeOf(plant)`               | **false**      |
| 3 | `getPrototypeOf(dog) === Dog.prototype`                       | **false**      |
| 4 | `getPrototypeOf(dog).constructor === Dog`                     | **false**      |
| 5 | `dog.constructor === Dog`                                     | **false**      |
| 6 | `getPrototypeOf(Dog.prototype) === Animal.prototype`          | **false**      |
| 7 | `getPrototypeOf(Animal.prototype) === Object.prototype`       | true           |
| 8 | `Animal.prototype.isPrototypeOf(dog)`                         | **throws**     |
| 9 | `Object.create(Dog.prototype) instanceof Dog / Animal`        | **false**      |
| 10| `setPrototypeOf(plain, Animal.prototype)`, then `instanceof`  | **false**      |
| 11| `new (getPrototypeOf(dog).constructor)("fido")`               | **not a Dog**  |
| 12| `Object.assign(Object.create(getPrototypeOf(dog)), dog)`      | **not a Dog**  |
| 13| `Dog === Dog` (both erased)                                   | **false**      |

Where it comes from:

* Every class instance answers the one string `"__smelt_proto:class"`
  (`crates/smelt-runtime/src/runtime/object_model.rs:125`, `smelt_prototype_sentinel`). So prototypes of unrelated
  classes are equal (2), and none is `Dog.prototype` (3). The sentinel's own prototype is `Object.prototype`, so
  the chain skips the base class (6).
* A class value is rebuilt at every read (`crates/smelt-codegen-rust/src/emitter/construct.rs:113`,
  `smelt_class_value(...)` mints a new `SmeltObject`). So it has no identity (13), and it has no `prototype`
  entry (3).
* `instance.constructor` answers an interned stub per class name (`reflection.rs:60`, `smelt_class_constructor`).
  That stub builds a bare `{__smelt_class: name}`, so it is not the class (5), and a rebuild through it runs no
  constructor (11).
* `Object.create("__smelt_proto:class")` stamps `__smelt_class: true`, a `Bool`, not the class name
  (`object_model.rs:97`). Name-based `instanceof` cannot match it (9, 12).
* `Object.setPrototypeOf` evaluates its arguments and does nothing
  (`crates/smelt-frontend-ts/src/lowering/stdlib/objects.rs:695`) (10).
* `isPrototypeOf` on an object answers a fixed `false` (`property_access.rs:322`). On a sentinel string it is
  not a function (8).

## 1. Representation

A class's prototype is an erased object, `SmeltUnknown::Object`, interned once per class in a thread-local table
keyed by the class's tag (the `__smelt_class` name):

```text
SMELT_CLASS_PROTOTYPES: tag -> SmeltUnknown::Object {
    "__smelt_method:__smelt_class_proto":  String(tag),
}
```

The table is filled lazily, so a class with no `.prototype` read in the program costs nothing.

* **Identity.** The table hands back the same object, so `getPrototypeOf(a) === getPrototypeOf(b)` holds
  exactly when `a` and `b` are instances of one class.
* **Chain.** The prototype's own `[[Prototype]]` is recorded in the existing identity-keyed link table
  (`SMELT_OBJECT_PROTOTYPES`, `object_model.rs:20-70`). That table is already "the ONE prototype link", which
  `getPrototypeOf`, `__proto__` and the function-target `instanceof` walk all read. The link points to the base
  class's prototype, or to `Object.prototype` (the `"__smelt_proto:object"` sentinel) for a root class. No
  second chain mechanism is added.
* **`constructor`.** It is the class value, which is interned in turn (below). So
  `getPrototypeOf(x).constructor === C`, and `C.prototype.constructor === C`. It is answered on read, not
  stored: a stored one would make the class value and its prototype a reference cycle that printing and
  structural equality would have to break.
* **Marker key.** The tag lets `Object.create(proto)` and `setPrototypeOf` recognise a class prototype without a
  table lookup. It is stored under the `__smelt_method:` prefix, which every own-key, `for...in`, JSON and
  equality filter already hides (the own-key filter is an explicit list, so a new `__smelt_` key would not be).

The class value is interned by tag in the same way (`SMELT_CLASS_VALUES`). Its `prototype` entry is the class
prototype, so `Dog === Dog` and `Dog.prototype === getPrototypeOf(dog)`.

* **Who builds it.** The first read of the class in the program builds the value, as today, and registers it.
  Every later read returns the registered value.
* **Classes that are never read as values.** Until the program reads the class, `.constructor` answers the
  per-tag stand-in it answers at the base (`smelt_class_constructor`). Nothing can compare against a class the
  program never reads, so identity is not observable there. Generating a builder per class instead would need
  each class's constructor adapter outside any function body, which the runtime slot cannot reach.
* **The class's base.** It comes from the generated `smelt_program_class(tag) -> Option<&'static str>` (`""` for a root class),
  filled through the runtime's `program_classes` slot (`runtime_prelude::program_class_arms`). Codegen knows
  every `MirClass::base`.

Per-evaluation classes keep today's behaviour. These are classes declared in a function or block that runs more
than once, which carry a `__smelt_class_frame` statics marker. Each evaluation is a distinct class in
JavaScript, so a tag-keyed table would merge them; they are not interned. They are recorded as a known gap, not
silently merged.

Two classes that share a source name in different modules already share a `__smelt_class` tag at the base
(`erase.rs`, `unknown_object_with_markers`). This design inherits that limit and does not widen it. The
generated `smelt_program_class` skips any tag that more than one class claims: such a tag is neither interned
(at the base each read is a fresh value) nor linked to a base.

### Typed where the class is known

Prototype objects are reflection: what a program does with one (read `.constructor`, compare it, pass it to
`Object.create`) is dynamic by nature. They stay erased. That is a real dynamic boundary, the same one
`SMELT_OBJECT_PROTOTYPES` already crosses, and it lives in the runtime prelude, which the SmeltUnknown report
classifies as prelude.

What stays typed:

* `x instanceof C` on a typed operand keeps folding statically (`emitter/instance_of.rs`).
* A typed class instance keeps its struct. Its prototype is reached by erasing it first, exactly as today.
* `new (x.constructor)(...)` on an erased instance now runs the real constructor. That rebuilds a typed struct
  and erases it once, instead of building an empty record.

No new `SmeltUnknown` reaches ordinary data flow.

## 2. Operations

| operation                         | new behaviour                                                                    |
|-----------------------------------|----------------------------------------------------------------------------------|
| `getPrototypeOf(x)`, `x.__proto__`| recorded link first (unchanged); else an object with a `String` `__smelt_class` tag answers `smelt_class_prototype(tag)`; else as today |
| `getPrototypeOf(C.prototype)`     | the recorded link: the base class prototype, or `Object.prototype`               |
| `C.prototype`                     | the `prototype` entry of the interned class value                                |
| `x.constructor` (class instance)  | the interned class value (the stub only for tags without a program class)        |
| `Object.create(classProto)`       | fresh object with `__smelt_class: String(tag)` (not `Bool`), link recorded        |
| `Object.setPrototypeOf(o, p)`     | erased object `o`: record the link; a class prototype also stamps the tag in place. Returns `o`. A typed target cannot change type: evaluated, no effect, as today |
| `p.isPrototypeOf(x)`              | walk `getPrototypeOf` from `x` comparing identity                                |
| `x instanceof C` (dynamic target) | name path unchanged; then the chain walk compares against `C.prototype`, so an object whose link was set by `create`/`setPrototypeOf` matches |

The `"__smelt_proto:class"` sentinel stays only as the answer for a legacy `__smelt_class: true` record. Nothing
new produces one.

## 3. `Object.assign` onto `unknown`

Today the in-place merge is limited to type-parameter targets
(`crates/smelt-frontend-ts/src/lowering/stdlib/object_assign.rs:522-575`). The reason: an erased Error or Blob
source enumerated keys that JavaScript does not (`name`, `message`, `stack`, `cause`), and es-toolkit's `clone`
broke. #445 made `name` conditional. Step 3 makes the error's remaining non-enumerable slots hidden from
`Object.assign`'s source walk, the same walk `Object.keys` uses. It then widens the in-place merge to
`unknown`/`any` targets.

es-toolkit's `clone` (`src/object/clone.ts`) is the acceptance test. It runs
`Object.create(getPrototypeOf(obj))` followed by `Object.assign(newObject, obj)`, and with step 1 that keeps
the class.

## 4. Steps (each shippable: tests green, ratchets at or below the base)

1. Runtime: class prototypes and interned class values. Codegen: `prototype` on the class value, and the
   generated base and class functions. `getPrototypeOf`, `constructor`, `Object.create`, `instanceof`. Fixture:
   the probe above (checks 1-7, 9, 11-13).
2. `setPrototypeOf` and `isPrototypeOf` (checks 8, 10).
3. Error slots hidden from the assign walk, and `Object.assign` onto `unknown` in place. es-toolkit runtime
   gate 1048/11 or better, `clone` tests included.

## 5. Acceptance criteria

* The probe matches Node on all 13 lines, as a Node-parity fixture.
* Each step has a defect injection that its fixture detects:
  * interning disabled (13 fails);
  * the base link dropped (6 fails);
  * the `Bool` tag restored in `Object.create` (9 fails).
* SmeltUnknown avoidable-erasure and typedness baselines do not rise, and neither does the clippy ratchet.
* es-toolkit runtime is at least 1048 passed with at most 11 pinned failures. date-fns and ts-pattern pins
  hold.

## 6. Known gaps after this design

* Methods are not on the prototype. An erased instance carries receiver-bound adapters under
  `__smelt_method:*`, so `a.greet === b.greet` is false, as is `C.prototype.greet`. This is es-toolkit's open
  `clone should clone custom classes` item. It needs `this`-taking functions on the prototype, and a read that
  binds the receiver. That is a follow-up.
* Per-evaluation classes are not interned (§1).
* Classes that share a source name keep the stub `constructor` (§1).
* `Blob.prototype` (the builtin prototype object) and the prototype `getPrototypeOf(blob)` reflects are two
  objects, so they compare unequal. `Date.prototype` read as a value does not compile at all. Both predate this
  design.
* `.constructor` read before the program reads the class as a value is the per-tag stand-in, so it is not `===`
  to the class read later. It builds through the class's real constructor when an instance recorded one.

## Implementation notes

* `Object.prototype` members read off an erased object (`proto.isPrototypeOf(x)`) ignored the `this` channel,
  which is where both a method call and `.call(receiver, ..)` put the receiver, so the argument was taken for
  the receiver. They now prepend the bound receiver when the program has a `this` channel.
* `Object.assign` onto `unknown` writes in place (`object_assign_erased_target`). The in-place merge copies
  own enumerable entries only, so a clone's methods, which Node finds on the prototype, were lost. Until
  methods live on the prototype (gap 1 below), a target of the source's own class takes the source's
  `__smelt_method:` adapters (`smelt_assign_class_methods`), which is what the copy path gave before.
* Defect injections, each detected by the `class_prototypes` Node-parity fixture: interning disabled,
  base link dropped, `Bool` tag restored in `Object.create`.

## 7. Closing the gaps (next PR)

Each item below is one of the known gaps, fixed by a general rule rather than a special case.

1. **Methods on the prototype.** Each class prototype holds its methods once, as `this`-taking erased
   functions under `__smelt_method:<name>`. They are generated per class, with the class's full emit context,
   and reached through the `program_classes` table. A read that misses an instance's own entries falls back to
   its class prototype, then to the base class's, so `a.greet === b.greet`, and a clone or an
   `Object.create(C.prototype)` object finds them. A read of a user-class method name off an erased value makes
   the program carry the `this` channel, as modeled builtin method names already do
   (`operand_may_read_prototype_method`). The per-instance receiver-bound adapters go away, and so does the
   copy in `smelt_assign_class_methods`.
2. **One class value per tag, from the first need.** The runtime owns the class value: an interned object per
   tag, created the first time anything asks for it (`.constructor`, `C.prototype.constructor`, or a read of
   `C` itself).
   * Its `instanceof` coverage (the tag and its subclasses) is computed from the `program_classes` table.
   * Its `__smelt_call` is the class's generated erased constructor.
   * The first read of `C` in program code fills in its statics in place, so the object handed out earlier is
     the class: `x.constructor === C` holds whatever the order. The per-tag stand-in function goes away.
3. **Host prototypes.** `X.prototype` for a host global `X` (`Date`, `Map`, `TypeError`, `Blob`, ...) lowers to
   one runtime helper answering one interned prototype object per host class. That object is the same one
   `Object.getPrototypeOf` reflects for an instance and `Object.create` links to. Its `constructor` is the
   host's constructor value, and its own prototype is the base host's (`TypeError.prototype` →
   `Error.prototype`) or `Object.prototype`. Today the builtin prototype object (`smelt_builtin_prototype_object`)
   and the reflected one (`smelt_reflected_prototype`) are separate caches. After this they are one.
4. **`Date.prototype` as a value.** It falls out of 3: the read no longer goes through the value of `Date`,
   which lowers to a default `SmeltDate`, so it no longer reads a struct field that does not exist.
