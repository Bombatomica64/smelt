# H68 — a class bound to a const was not a class

Round 27, item 1. Found while writing H44's guard; **fixed**. Three silent
wrong values in one shape.

## The measurements

```ts
const Boxed = class<T> { items: T[] = []; push(item: T): number { … } };
const numbers = new Boxed<number>();
```

| source | JavaScript | before |
| --- | --- | --- |
| `numbers.push(7)` | `1` | **`null`** |
| `numbers.items[0]` | `7` | **`undefined`** |
| `numbers instanceof Boxed` | `true` | `true` |
| `typeof Boxed` | `function` | **`object`** |
| `const R = class extends Counter { report() { return this.tally() } }` then `new R().report()` | `a,b` | **`null`** |
| the same class DECLARED (`class R extends Counter`) | `a,b` | `a,b` — correct |

## Why

`const Foo = class { … }` lowered the class (into a synthesized
`__smelt_anon_class_N`) and gave the BINDING a placeholder value typed as that
class. So the const held `Default::default()` — a default INSTANCE standing in
for the class — and `new Foo()` did not resolve to a class item at all: it fell
through to the nominal-name path, and the emitter turned it into
`smelt_construct(<the placeholder instance, erased>)`, after which every member
went through `smelt_get_unknown_field(&counter, "add")` dynamic dispatch and
answered nothing.

## The rule

**A class expression that initializes a binding is a class declaration under
that binding's name.** That is the name TypeScript itself infers, and it is what
every nominal use of the binding depends on: `new Foo()`, `Foo` in type
position, `x instanceof Foo`, `extends Foo`.

- `const_class_expression` matches the declarator (unwrapping `as`/`satisfies`/
  parentheses exactly as its `const_constructor_function` sibling does);
- `program_class_names` collects those names in the prepass, so a `new Foo()`
  lowered earlier in the module still resolves nominally;
- the declarator lowers as a class declaration and contributes NO runtime
  binding — the same shape the constructor-function case already had;
- `class_expression_binding_name` carries the name into `class_declaration`,
  which consumes it, so a class expression nested deeper in the same
  initializer still takes its synthetic anonymous name.

## `typeof` of a class reference

`typeof Boxed` answered `object` — and so did `typeof DeclaredReporting`, so
this half was never about class expressions. HIR types a class and its
instances alike as `Type::Class`, and `typeof_type_name` answers `object` for
it, which is right for an instance and wrong for the class: a class reference
is a function value in JavaScript.

The distinction has to come from the operand's SPELLING, so `typeof_expression`
now folds a class NAME (not shadowed by a local) to `"function"`, generalizing
the check one line above it that already did this for modeled host
constructors (`typeof Blob`). `typeof new Foo()` keeps answering `object`.

## Guarded by

`examples/typescript/end-to-end/82_class_expression_binding`, verified to fail
with the frontend change reverted. It covers the extending class expression
(which is also H44's shape, so **H44's guard is now a runtime assertion** as
well as its codegen test), a declared control, a generic class expression
constructed with an explicit type argument, `instanceof`, and all three
`typeof` answers.

## Found on the way

The synthesized constructor of a derived class with NO explicit constructor
takes one `Option<SmeltUnknown>` "super argument"
(`synthesize_default_class_constructor`), which is 4 avoidable-erasure lines per
such class. It is deliberate — it keeps Date-like subclass construction
call-compatible — but where the base constructor takes no parameters it erases
for nothing; deriving the parameter list from the base's own constructor is the
honest rule. Not in this commit: the fixture spells `constructor() { super(); }`
so the examples invariant stays 0 by construction. Numbered **H69**.
