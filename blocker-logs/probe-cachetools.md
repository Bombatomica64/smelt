# Probe: cachetools (first scoping pass, 2026-11-04)

## Reproduction

- Source: `tkem/cachetools` at `340c474a11bdbdce46c9c6ff19ec9c693d038a4d`
  (tag `v7.2.8`, released 2026-10-31).
- Fixture: `.github/compat/cachetools/Smelt.toml`. It uses roots `src` and
  `tests`, entry `src/cachetools/__init__.py`, and no `test-prefix` and no
  excludes, as the first-pass rule asks.
- Smelt: this branch's `target/debug/smelt`, built from `ba730229` with the
  default features (`ty`, `ts7`).
- Commands:
  - `smelt --manifest-path <lib>/Smelt.toml probe --format md`
  - `smelt --manifest-path <lib>/Smelt.toml build`
  - `smelt dump-hir <file>` run on each of the 20 `.py` files, to get the
    complete per-file census below. `probe` reports only the files that
    the manifest pass flags.

Corpus size: 6 library modules (`__init__`, `_cached`, `_cachedmethod`,
`_descriptor`, `func`, `keys`; about 1,400 lines) and 14 test modules. The
package also ships `py.typed` and hand-written stubs `__init__.pyi`,
`func.pyi` and `keys.pyi`, 420 lines in total.

## How far it transpiles

**It does not transpile.** `smelt build` aborts in the Python frontend at
`src/cachetools/_cached.py`, before any MIR or Rust is produced. Because no
crate is emitted, `rust-test-report` cannot run.

`smelt probe` reports 20 files scanned and 4 with blockers: 34 occurrences,
28 of them `type-constraint` and 6 `unsupported-lowering`. Running
`dump-hir` on each file is more complete: **all 20 files fail HIR
lowering, with 81 blocker sites** (42 in the library, 39 in tests). No
library module lowers cleanly:

| module | blockers |
| --- | ---: |
| `src/cachetools/__init__.py` | 11 |
| `src/cachetools/_cachedmethod.py` | 9 |
| `src/cachetools/_cached.py` | 9 |
| `src/cachetools/func.py` | 7 |
| `src/cachetools/keys.py` | 5 |
| `src/cachetools/_descriptor.py` | 1 |
| 14 test modules | 39 |

Where cachetools sits compared with the existing Python probes
(`blocker-logs/library-probes.md`): the same untyped-signature wall stops
toolz and funcy, but here it is smaller. cachetools has 81 sites against
toolz's 200+. The new families are class-shaped: inheritance, nested
classes, and decorated methods. That is what this corpus was added to
expose.

## Blocker families (per-file `dump-hir` census, 81 sites)

| # | sites (src / tests) | family | example |
| ---: | ---: | --- | --- |
| 1 | 44 (35 / 9) | Unannotated signatures `ty` cannot type: `function 'X' must have an explicit return type annotation` (21), `parameter 'X' in 'C.m' must have a type annotation` (14), `parameter 'X' must have an explicit type annotation` (9) | `_cached.py:193 def _locked(func, cache, key, lock):` returns a nested `wrapper` closure that has a function attribute assigned (`wrapper.cache_clear = cache_clear`) |
| 2 | 16 (0 / 16) | `class 'X': multiple inheritance is not supported` | `tests/test_lru.py:8 class LRUCacheTest(unittest.TestCase, CacheTestMixin):` |
| 3 | 8 (1 / 7) | `class 'X': complex base class expression not supported` (dotted base) | `__init__.py:27 class Cache(collections.abc.MutableMapping):`, `tests/test_keys.py:6 class CacheKeysTest(unittest.TestCase):` |
| 4 | 4 (4 / 0) | `class 'X': unsupported class body statement 'class'` (nested class) | `__init__.py:210 class LFUCache(Cache): class _Link: __slots__ = (...)` |
| 5 | 3 (0 / 3) | `class 'X': decorator '@cachedmethod' is not supported` (decorator factory on a method) | `tests/test_cachedmethod.py:22 @cachedmethod(lambda self: self.cache)` |
| 6 | 2 (0 / 2) | `only calls to top-level functions, class constructors, and print() are supported` | `tests/__init__.py:69 cache = self.Cache(maxsize=1)` (calling a class held in an attribute) |
| 7 | 1 | `unresolved name 'collections'` | `__init__.py:745 _CacheInfo = collections.namedtuple("CacheInfo", [...])` |
| 8 | 1 | `definition-time metaprogramming for '_MethodDescriptor' requires host-runtime specialization` | `_descriptor.py:25 class _MethodDescriptor:` (`__set_name__`/`__get__` descriptor protocol) |
| 9 | 1 | `derives from typing.Protocol: structural subtyping is not supported` | `tests/__init__.py:41 class SimplifiedTestCaseProtocol(Protocol):` |
| 10 | 1 | `Callable first argument must be a list of param types` | `tests/test_func.py:11 DECORATOR: Callable[..., Any]` |

Several of these hide others. Families 2 and 3 reject a whole class before
its methods are checked, so once they are lifted the test mixins (for
example `CacheTestMixin`, about 300 lines) will show more family-1 and
family-6 sites. Treat the test-side counts as lower bounds.

### Top 3 for cachetools

1. **Unannotated signatures (44 sites).** This is the main blocker in the
   library itself: 35 of the 42 library sites.
2. **Multiple inheritance with a mixin (16 sites).** This blocks every test
   class.
3. **Dotted base classes such as `collections.abc.MutableMapping` and
   `unittest.TestCase` (8 sites).** It blocks the root `Cache` class and the
   remaining test classes.

## General rule or one-off gap?

- **Family 1 needs a general rule, and the fix has a clear starting point.**
  `ResolvedTypes::resolve` (`crates/smelt-frontend-py/src/ty_resolve.rs`)
  calls `smelt_py_types::resolve_module_types(source, stem)` on each module
  in isolation. Because of that, `ty` never sees the package's own
  `__init__.pyi` / `func.pyi` / `keys.pyi`, which fully annotate the public
  API: `Cache(MutableMapping[_KT, _VT])`, generic `cached(...)`, and so on.
  It also never sees argument types flowing in from sibling modules. Two
  general rules would cover this, and neither is cachetools-specific:
  1. Resolve with `ty` across the whole manifest project, not one module at
     a time.
  2. When a `.py` module has a PEP 561 sibling `.pyi`, use the stub's
     signatures.

  The rest of the family (private helpers such as `_locked(func, cache,
  key, lock)` that have no stub entry) needs call-site-driven
  specialization of unannotated parameters. A hand-written Rust port would
  make these generic: `F: Fn(Args) -> R`, `C: MutableMapping<K, V>`.
- **Function attributes on closures** (`wrapper.cache_clear = ...`,
  `wrapper.cache_info = ...`) sit inside family 1 and are the real
  modelling question. A Rust team would write a struct that holds the
  closure state, with `call` and `cache_clear` methods. That is general (it
  is the shape of `functools.lru_cache`'s result) and is not a one-off.
- **Families 2 and 3 are general class-model rules.** Multiple inheritance
  with stateless mixins maps to a trait with default methods. Resolving a
  dotted base through its module path is the Python counterpart of the
  "qualified type references must preserve the full alias path" rule in
  `CLAUDE.md`. `collections.abc.MutableMapping` as a base also needs the
  ABC's mixin methods (`get`, `pop`, `setdefault`, `update`, ...) to be
  supplied from the five abstract ones.
- **Family 4 (nested classes) is general.** Hoist the nested class to a
  module-level type with a qualified name. `__slots__` is a layout hint
  that becomes the struct's field list.
- **Family 5 (method decorator factories) is general** once family 1 lets
  `cachedmethod(...)` have a type. Today every decorator other than the
  built-in ones is rejected on methods.
- **Families 7 and 10 are small general gaps.** `collections.namedtuple`
  reached through an `import collections` attribute is unresolved, and
  `Callable[..., T]` (the ellipsis form) is unsupported.
- **Family 8 (the descriptor protocol, `__set_name__` / `__get__`) is
  probably a real dynamic boundary**, or at least the hardest item here. It
  backs `cachedmethod`'s per-instance cache lookup. It needs a design
  decision rather than a lowering patch.
- **Family 9 (`typing.Protocol`) is general and already known**:
  structural subtyping maps to a trait.

Nothing here looks like a cachetools-only quirk. Each family is a Python
class-model or typing rule that the existing functional-style probes
(toolz, funcy, more-itertools) simply did not exercise.

## Not covered yet

- No generated Rust: there is no `rust-test-report` and there are no
  typedness or `SmeltUnknown` numbers, because the build stops in the
  frontend.
- `test_threading.py` uses real threads, locks and conditions. Even after
  lowering, it will need the runtime's threading model, so check it
  separately.
- No CI gate or baseline was added. The fixture is only in
  `.github/compat/libraries.json`, so the daily `Library Probes` run will
  track it.

## Class-model pass (branch `py/cachetools-class-model`, 2026-11-04)

The same per-file `dump-hir` census, re-run after each family. The total
stays at **81 sites in 20 files** (42 library, 39 tests). 12 sites are
cleared, and lifting them exposes 12 sites that were hidden behind them.

| family | before | after | change |
| --- | ---: | ---: | --- |
| 3. dotted base classes (`collections.abc.MutableMapping`, `unittest.TestCase`) | 8 | 0 | general rule: a dotted base resolves through the module's import bindings, as `from m import Name` does |
| 4. nested classes in a class body | 4 | 0 | general rule: hoisted to a module-level type named by `__qualname__` (`LFUCache._Link`, Rust `LFUCache__Link`) |
| hidden blockers now exposed | – | 12 | listed below |

Exposed by family 3 (the class body or base now gets checked):

- `Cache`: `__marker = object()` is a class-level assignment that is not a
  literal.
- Test classes on `unittest.TestCase`: `unknown class field 'subTest'` /
  `'cache'`. `TestCase` is recorded as a base, but it is not modeled, so its
  methods are unknown. Also more family-1 sites (`CacheKeysTest.test_hashkey`,
  `NoneMethodTest.cache`, `NoneWrapperTest.func`), plus a nested class inside
  a test *function* body and an attribute-held class call (family 6).

Exposed by family 4 (the hoisted class bodies are now lowered):

- 3 family-1 sites: `LFUCache._Link.__init__(count)`,
  `TTLCache._Link.__init__(key)` and `_TimedCache._Timer.__init__(timer)`.
- `TLRUCache._Item` is decorated with `@functools.total_ordering`.
- Not reported yet: once their `__init__` is typed, `_Link` will still need
  `__slots__` as its field list, because `next`/`prev` are only assigned
  from outside the class.

Census now (81 sites): unannotated signatures 50 (21 return, 19 method
parameter, 9 parameter, 1 method return), multiple inheritance 16, class
decorators 4 (3 `@cachedmethod`, 1 `@functools.total_ordering`),
attribute-held class calls 3, unknown `TestCase` members 2, and one each of:
`collections` unresolved, nested class in a function, descriptor
metaprogramming, `typing.Protocol`, `Callable[..., T]`, and a non-literal
class attribute.

### Families left for a decision

- **Family 2 (multiple inheritance, 16 sites) is blocked on two other
  families.** Every mixin (`CacheTestMixin`, `DecoratorTestMixin`,
  `MethodDecoratorTestMixin`) derives from `SimplifiedTestCaseProtocol`, a
  `typing.Protocol` that declares the `assert*` methods the mixin calls on
  `self`. In Rust that is a trait with a supertrait bound, which is
  structural subtyping (family 9). Every concrete class also pairs the mixin
  with `unittest.TestCase`, which is not modeled. Lifting the
  multiple-inheritance check alone would only turn these 16 sites into
  `unknown class field 'assertEqual'`.
- **Families 7 and 10 (`collections.namedtuple`, `Callable[..., Any]`)
  are not reference gaps.** `namedtuple(...)` in its functional form is not
  supported at all, whatever its spelling, and its fields are untyped.
  `Callable[..., Any]` is the gradual callable. Both need a decision on
  what the Python frontend does with source `Any` and untyped fields. Today
  it has no erasure path, and `Any` lowers as a nominal class. The other
  option is to take the types from the PEP 561 stub (`__init__.pyi`
  declares `class _CacheInfo(NamedTuple)` with typed fields).
- **Family 1 (unannotated signatures, 50 sites) is a redesign.** Resolving
  with `ty` across the manifest project would type imports through the
  package stubs. Taking the module's own signatures from its sibling `.pyi`
  would cover about 14 of the 35 library sites (the public `__init__`s,
  `cached`, `cachedmethod`, `*_cache`, `*key`). Those stub signatures use
  `Callable[..., _KT]`, TypeVars and `@overload`, so they meet the
  `Callable[..., T]` question too. The remaining private helpers
  (`_locked(func, cache, key, lock)` and similar) need call-site-driven
  generic specialization, plus a struct for closures that carry attributes
  (`wrapper.cache_info = ...`).

### Known limit of the nested-class rule

A hoisted class's Rust name sanitizes `.` to `_`, so a nested `A.B` and a
module-level class literally named `A_B` would collide. This is the same
scheme the TypeScript frontend uses for namespace members.

Separately, the end-to-end check for nested classes found a pre-existing bug
that is not tied to nesting: a module-level instance (`c = Counter()` at
module scope) is rebuilt at each use in the generated Rust, so mutations
through it are lost. It reproduces without nested classes.
