# Python gradual typing: taxonomy and plan

Status: proposal, 2026-11-04. Written for the cachetools probe
(`blocker-logs/probe-cachetools.md`). The rule asked for was "lower Python's
gradual constructs the way TypeScript's are lowered; where that is not
possible, write a plan and a full taxonomy". This document is that
taxonomy and plan.

## What was tried

`typing.Any` and `Callable[..., R]` were prototyped with the TypeScript
lowering:

- `Any` erases to `Type::Unknown` with an `explicit-any` reason.
- `Callable[..., R]` lowers to the erased-rest function shape, which is
  TypeScript's `(...args: any[]) => R`.

The shapes lower and compile. Calling through `Callable[..., int]` printed
the same as CPython once a shared codegen bug was fixed (see below). But the
end-to-end check also showed why `Any` cannot reuse the TypeScript *runtime
value*:

```python
def show(v: Any) -> None:
    print(v)

show(True)   # CPython: True    generated Rust: true
```

`SmeltUnknown` is JavaScript's value model. Its whole definition is in
`smelt-codegen-rust/src/lib.rs`, `pub enum SmeltUnknown`:
`Null | Undefined | Bool | Number(f64) | BigInt | String | Symbol | Array |
Object | Function | Promise`. It prints and compares with JavaScript rules.
A Python value that passes through it loses meaning in the ways listed in
the next table.

### Shared bug fixed on the way

A callable parameter with the erased-rest shape is a borrowed
`&dyn Fn(SmeltList<SmeltUnknown>) -> R` handle. It returns its declared `R`.
The emitter, however, re-extracted that `R` as if it were the result of the
owned `SmeltErasedFunction::call` method, which does return `SmeltUnknown`.
So every TypeScript `(...args: any[]) => number` callback parameter produced
E0308. The fix is in `emitter/call_runtime.rs`, and
`tests/erased_rest_callback_tests.rs` covers it.

## Python value semantics `SmeltUnknown` cannot carry

| Python | JS-shaped `SmeltUnknown` | consequence |
| --- | --- | --- |
| `int`, arbitrary precision | `Number(f64)` | values above 2^53 are rounded; `int` and `float` become indistinguishable |
| `float` (`str(3.0) == "3.0"`) | `Number(f64)`, prints `3` | wrong `print` / `str` |
| `bool`, a subclass of `int` (`True + 1 == 2`; prints `True`) | `Bool`, prints `true` | wrong `print`; `bool` arithmetic needs an explicit rule |
| `None`, one value | `Null` and `Undefined` | two representations of one value |
| `tuple`: immutable, hashable, `(1, 2) != [1, 2]` | `Array` | tuple and list identity is lost |
| `dict` with any hashable keys | `Object`, string keys | `d[1]` and `d["1"]` collide |
| `set` / `frozenset` | none | not representable |
| class instance: nominal, `isinstance` | `Object` record | the class identity has to be re-encoded |
| `==` / `is` / `hash` | `js_strict_eq` / reference id | different equality rules |
| `str()` vs `repr()` | one JavaScript `console.log` format | `repr` is missing |

So the TypeScript *lowering* (an erased slot, plus a codegen adapter where a
concrete value enters it) carries over. The TypeScript *value model* does
not.

## Taxonomy of Python's gradual and dynamic constructs

"Same as TS" means the existing TypeScript lowering applies unchanged.

| # | Python construct | TS analog | Smelt today | can it be lowered like TS? |
| ---: | --- | --- | --- | --- |
| 1 | `x: Any` (param, field, return, local) | `any` | nominal `Class("Any")`; fails later | the shape, yes; the value model, no (see above) |
| 2 | `x: object` (top type: only `object`'s methods allowed) | `unknown` | nominal `Class("object")` | yes, as `source-unknown`, a legitimate boundary. Python allows only `object`'s own operations on it, so the value is only stored and passed on; the value-model gap matters only for printing |
| 3 | unannotated parameter or return that `ty` cannot infer | implicit `any` | error `must have an explicit … annotation` | shape yes (`missing-annotation`), but this is family 1 of the probe. Infer from call sites first (item 3: project-wide `ty`); erase only as the last resort |
| 4 | `Callable[..., R]` | `(...args: any[]) => R` | prototyped on branch `py/gradual-any`, not shipped | shape yes, and it runs. But each argument crosses the erased call as a JS-shaped value, so a Python `int` above 2^53 is rounded on the way in (TypeScript numbers are `f64` already, so TS loses nothing). It therefore depends on the same decision as #1 |
| 5 | `*args` / `**kwargs` without annotation | `...args: any[]` | erased (`missing-annotation`, `list.rs` for lambdas) | yes |
| 6 | `collections.namedtuple("P", ["x", "y"])` (untyped fields) | none: fields are implicit `any` | unsupported (`unresolved name` / not a constructor) | fields: like #3. The record itself has no TS analog (see #7) |
| 7 | `typing.NamedTuple`, class or functional form | none: a record that is also a tuple | class form lowers as a plain class with a **no-argument** constructor (latent bug) | no. Needs a Python rule: a frozen record with positional construction, plus the tuple protocol (`t[0]`, unpacking, `== (…)`, `len`, iteration) |
| 8 | `TypedDict` | object type / interface | not checked | yes: structural record = TS interface |
| 9 | `dict[str, Any]` (JSON-like) | `Record<string, any>` | value is the `Any` of #1 | shape yes; value model as #1 |
| 10 | `typing.cast(T, x)` | `x as T` | not checked | yes: a checked cast (`UnknownCast`) when `x` is erased, otherwise a no-op |
| 11 | `isinstance(x, C)` narrowing an `Any`, `object` or union | `typeof` / `instanceof` narrowing | not checked | yes for unions; for erased values it needs the Python class identity in the value model |
| 12 | `type[C]` and class objects as values (`self.Cache(maxsize=1)`) | `new () => T`, `typeof C` | error (`only calls to top-level functions…`) | yes: TS's constructor-value lowering |
| 13 | `typing.Protocol` (structural) | `interface` (structural) | rejected (`smelt::no-protocol`) | **yes**: Smelt already lowers TS interfaces structurally to HIR `Interface` + `implements`. This is the rule item 1 uses |
| 14 | `getattr` / `setattr` / `hasattr` with a computed name | `obj[key]` | not checked | shape yes (`by-name`); value model as #1 |
| 15 | function attributes (`wrapper.cache_info = f`) | function with properties (`F & { cache }`), the callable-object record with a `__smelt_call` slot | error | **yes**: the TS callable-object lowering (`decls/callable_object.rs`) |
| 16 | decorator factories returning wrappers (`@cachedmethod(...)`) | higher-order functions | rejected on methods | yes, once the wrapper's type is known (#3, #15) |
| 17 | descriptors (`__get__` / `__set_name__`), metaclasses | none | specialization / rejected | no: host-runtime specialization (out of scope, as agreed) |

## Decision needed: the value behind `Any`

Rows 1, 9, 11 and 14 need a Rust value for a Python value whose type is
genuinely unknown. The options:

- **A. Extend `SmeltUnknown` with Python tags.** Add `Int(i64)` / `BigInt`,
  `Tuple`, a Python `None`, and a Python formatter chosen by source
  language.
  - For: one runtime, and the reports already understand it.
  - Against: every JavaScript helper (`js_strict_eq`, the coercions, the
    console formatter, ~73k lines of prelude in the examples baseline) has to
    handle tags that JavaScript never produces. The JS runtime would also
    take on Python semantics.
- **B. A separate Python dynamic value** (`PyAny`: `Int`, `Float`, `Bool`,
  `Str`, `None`, `Bytes`, `List`, `Tuple`, `Dict`, `Set`, `Instance(Rc<…>)`,
  `Callable`), with Python `==`/`hash`/`str`/`repr`.
  - For: exact semantics. This is what a hand-written port uses for genuinely
    dynamic data.
  - Against: a second runtime, per-type adapters in codegen, and the unknown
    and typedness reports have to learn it.
- **C. No runtime `Any` where the use allows it: specialize.** An `Any` that
  is only passed through, or is used through operations a trait bound
  expresses, becomes a scoped generic. For example
  `assertEqual(first: Any, second: Any)` becomes
  `fn assert_equal<A: PartialEq<B> + Debug, B: Debug>`. A value that is
  inspected dynamically (`isinstance`, `getattr` with a computed name) falls
  back to A or B.
  - For: the CLAUDE.md north star ("prefer `T: Trait` over erasure"). The
    cachetools uses (`Protocol` test helpers, `Callable[..., Any]` decorator
    slots) are all pass-through.
  - Against: needs use-site analysis, which is close to the call-site
    specialization that item 3 needs anyway.

**Recommendation: C first, with B as the fallback for genuinely dynamic
values.** C keeps the typedness ratchet honest: an `Any` that a generic
carries counts as *generic*, not *erased*. B keeps Python semantics exact
where erasure is real. A is the cheapest option, but it bends the JavaScript
runtime around Python and still leaves `print(True)` to a per-language
switch.

Until this is decided, `Any` stays unsupported. It still lowers as the
nominal `Class("Any")` and fails downstream; a deliberate diagnostic would
be better. Rows 13 and 15 do not depend on the decision. Row 4 does: under C it becomes a generic `F` callable, specialized per call site.
