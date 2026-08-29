# Python frontend blocker triage (issues #93 / #94 / #95)

_Investigation notes from the `claude/python-issues-xwbrk5` branch. Probe counts
are from `blocker-logs/library-probes.md` (generated 2026-08-25); reproductions
were run against a locally built `smelt dump-hir` after the Ruff/ty 0.0.10
upgrade._

## Method for reproducing

Each case below is a minimal `.py` file run through
`smelt dump-hir <file>` inside a scratch project carrying a `Smelt.toml` with
`[strict] python = true`. Note that `dump-hir` prints diagnostics inside a Rust
`Debug` string, so the message text is **escaped** — grep for `message: \\"`,
not `message: "`, or every failing case reads as a pass.

## Where issue #93 (ty-resolved types) stands

Return types are resolved through `ty` for top-level functions and methods, and
(as of this branch) nested closures no longer need an annotation at all. The
remaining hole is **parameter** types:

| Shape | Result |
| --- | --- |
| `def inc(x: int):` (unannotated return) | resolves via `ty` |
| `def m(self):` on a class (unannotated return) | resolves via `ty` |
| `def add(x: int):` nested closure (unannotated return) | resolves from the lowered body (this branch) |
| `def inc(x):` (unannotated parameter) | still an error |
| `def inc(x=0):` (unannotated parameter with a default) | still an error |

Confirmed unchanged by the ty 0.0.10 upgrade: `ty` resolves a parameter's type
from its *annotation*, not from a default value or from call sites, and reports
an unannotated parameter as dynamic. `displayable_type` drops dynamic results on
purpose, so those stay an explicit boundary. Closing the remaining
`parameter 'X' must have an explicit type annotation` family (123 occ across 4
libraries) needs call-site or default-value inference that `ty` does not offer —
it is not a version-bump away.

## Where issue #94 (method / non-top-level calls) stands

`only calls to top-level functions, class constructors, and print() are
supported` is still the single largest Python blocker (683 occ across all 5
probed libraries), but it is no longer one gap — most method-call shapes already
lower. Reproductions:

| Shape | Result |
| --- | --- |
| `x.a()` where `x: A` is a parameter | lowers |
| `y = x; y.a()` | lowers |
| `make().a()` (chained on a call result) | lowers |
| `self.a()` (sibling instance method) | lowers |
| `self.a()` inherited from a base class | lowers |
| `Class.static()` / `cls.helper()` / `cls(...)` | lowers |
| calling a nested `def` | lowers |
| `Callable[...]`-typed parameter called | lowers |
| `s.strip().upper()`, `xs.count(1)`, `d.get(k, v)` | lowers |
| **`self.field.method()` where `field` is typed only by the `__init__` parameter** | **blocked** |
| **`super().__init__(...)`** | **blocked** |

Two concrete, general sub-gaps remain:

1. **Implicit field declaration.** Method dispatch resolves through a field only
   when the class carries a class-level annotation for it:

   ```python
   class B:
       inner: A                      # <- present: `self.inner.a()` lowers
       def __init__(self, inner: A) -> None:
           self.inner = inner
   ```

   Drop the `inner: A` line and the identical `self.inner.a()` fails, because
   the field's type is only recoverable from the `__init__` parameter's
   annotation. Assigning `self.<name> = <param>` in `__init__` should declare
   the field with the parameter's type, exactly as the annotated form does. Not
   a special case — it is the general rule Python itself uses. No existing test
   covers the unannotated form (every class test in `class_tests.rs` writes the
   class-level annotation), which is why it survived.

   **Fixed.** A pre-pass now declares a field for `self.<name>: T = <value>`
   and `self.<name> = <param>` in `__init__`, before the class body walk so
   every method sees them.

2. **`super()`.** There was no lowering for `super()` as a call receiver, so
   `super().__init__(x)` — the single most common line in any subclass
   `__init__` — fell through to the catch-all diagnostic.

   **Fixed** for the constructor case; see the inheritance section below.

## Inheritance under class flattening

Investigating `super()` surfaced that Smelt's class *flattening* — a subclass
struct stores its base's fields inline (`effective_class_fields`), with no base
value to delegate to — was only half-implemented on the method side.

**Fixed: inherited method calls emitted non-compiling Rust.** The frontend
resolves `self.<method>()` up the base chain (`class_method_item_by_name`), so
this lowered happily:

```python
class A:
    def fetch(self) -> int: return self.x
class B(A):
    def total(self) -> int: return self.fetch() + self.y
```

but codegen emitted only each class's *own* methods into its `impl` block, so
`fetch` lived in `impl A` and the generated `self.fetch()` on a `&B` failed with
`no method named 'fetch' found for reference '&B'`. `effective_class_methods`
now mirrors `effective_class_fields`: base methods first, then the class's own,
an override replacing the inherited slot in place. Re-emitting a base body under
the subclass is sound precisely because of flattening — every field it touches
exists on the subclass, and any sibling method it calls is inherited by the same
rule.

**Fixed: `super().__init__(..)`.** The TypeScript frontend already solved this
for `super(...)` and emits only ordinary HIR — no dedicated node. The Python
frontend now does the same (`super_init_statement`):

```text
let __smelt_super: Base = Base(args);   // the base's own constructor
self.<field> = __smelt_super.<field>;   // for each inherited field
```

Because the base is built through its *own* constructor, everything that
constructor does runs exactly once and in order, including its own
`super().__init__(..)`. Multi-level chains therefore need no special handling —
verified end to end: a three-level `A -> B -> C` chain compiles and returns the
right value at runtime.

It is intercepted at *statement* level, where the enclosing block is known, so
the emitted `let`/assignments land in the right block rather than the function
root.

**Constructor follow-up status.**

* `super().<method>()` on an ordinary method now lowers through a typed,
  collision-free alias of the immediate base implementation emitted on the
  flattened derived class. This bypasses an override without constructing an
  erased base object or recursing back into the override.
* A subclass with **no** `__init__` now inherits its base constructor signature.
  Smelt synthesizes a derived constructor that builds the concrete base and
  copies its flattened fields, matching explicit `super().__init__` lowering.

## Constructor-assigned field types

The declaration pass types an `__init__`- or `__new__`-assigned field only from
values whose type is knowable *before* any method body is lowered: a reference
to an initializer parameter, a scalar literal, or a constructor call for a
class the module already registered (`self.inner = Inner()`). Anything else — a call
result, an operator expression, an empty container — is left undeclared rather
than guessed. An undeclared read is a hard `unknown class field` rather than a
silently fabricated receiver type.

## Removed silent-wrong-type path

The old `field_type` fallback assigned the receiver's class type to every
unknown attribute on a fieldless class. It has been removed: undeclared reads
now report `unknown class field`. To avoid losing valid constructor-defined
shape, the declaration pre-pass also recognizes typed assignments on the local
instance returned by `__new__` (as well as assignments on `self` in `__init__`).

## Other shapes worth noting

* `g(1, b=3)` against `def g(a: int, b: int = 2)` reports
  `function keyword arguments require a **kwargs parameter`. Keyword arguments
  matched to *declared* parameters are ordinary Python and are a separate,
  self-contained gap in `callable_call_args`.
* Once the nested-closure annotation gate is removed, closures fall through to
  the next real limit — `callback expression is not supported yet` — because a
  nested closure body must classify into the compact `CallbackExpr` IR. That IR's
  coverage, not the annotation, is now the binding constraint for issue #95's
  closure work.
