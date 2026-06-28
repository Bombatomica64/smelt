# Python metaprogramming support — explicit plan

**Status:** design plan. Metaprogramming is **off by default today** and stays
that way; everything below is opt-in and staged. This document is the explicit
roadmap for *if/when* we want to support more of it.

---

## 0. Where we are today (ground truth)

Smelt's Python frontend is **reject-by-default with a tiny static whitelist** —
the same philosophy mypy/ty use, but stricter. Concretely:

- **Class decorators** — only `@dataclass` / `@dataclasses.dataclass` is accepted
  and lowered concretely (it knows dataclass generates `__init__` + fields + eq;
  `frozen=` is read via `helpers::decorator_frozen_kwarg`). Everything else →
  `SmeltError::unsupported_decorator`.
  - Code: `crates/smelt-frontend-py/src/lowering/class.rs:19-27` (decorator match
    → `unsupported_decorator`), and `class.rs:431` (a second decorator check).
- **Metaclasses** — `class X(metaclass=M)` → hard error
  `SmeltError::no_metaclass` (`lowering/class.rs:52`).
- **Django models / descriptor protocol** — explicit hard error
  `SmeltError::django_unsupported` (`lib.rs:162`).
- **Function decorators** — matched ad-hoc in `lowering/function.rs` (e.g.
  `fixture`, `parametrize` for tests); a scattered set of `match
  decorator_simple_name(...)` arms (`function.rs:58`, `:117`).
- **Dynamic attribute access** (`getattr`/`setattr`/`__getattr__`/`__setattr__`)
  — **not modeled at all**; lowers as ordinary calls/attribute access and fails
  on anything dynamic.
- The error helpers are all in `crates/smelt-frontend-py/src/lib.rs`
  (`unsupported`, `no_metaclass`, `unsupported_decorator`, `django_unsupported`),
  category `DiagnosticCategory::UnsupportedLowering`, code `smelt::unsupported-py`
  / `smelt::no-metaclass`.

The decorator handling is therefore already a **hand-coded pattern registry**,
just small (one real entry: `@dataclass`) and scattered across `class.rs` /
`class_init.rs` / `function.rs`.

---

## 1. The core constraint

Smelt is **AOT → concrete Rust** with static field layout and monomorphized
dispatch. Metaprogramming is *code that runs (at definition or runtime) to change
what classes / attributes / functions exist*. Support is feasible exactly to the
degree the result is **fixed and knowable before runtime**.

### Tiers (the whole plan is organized around these)

| Tier | What | Examples | Verdict |
| --- | --- | --- | --- |
| **1** | Definition-time, deterministic — effect depends only on the code as written | `@dataclass`, attrs, `Enum`, `NamedTuple`, `@property`, `@staticmethod`/`@classmethod`, `functools.lru_cache`, `@abstractmethod`, `@total_ordering`, `__init_subclass__`/`__set_name__`, most custom metaclasses | **Supportable** (Phases 1 & 3) |
| **2** | Runtime-dynamic but bounded — shape known, values dynamic | `getattr`/`setattr` with runtime keys, `__getattr__`/`__setattr__` fallbacks, monkeypatching a known attr set | **Supportable via dynamic representation** (Phase 2) |
| **3** | Unbounded dynamic | `exec`/`eval`/`compile`, `type(name, bases, ns)` from runtime data, `importlib`/`sys.modules` munging, runtime-built class bodies | **Permanently rejected** |

---

## 2. Config flag (Phase 0, prerequisite for everything)

Add to `Smelt.toml`:

```toml
[python]
# off       — current behavior: reject all metaprogramming except @dataclass
# patterns  — Phase 1: recognized decorator/metaclass registry
# dynamic   — Phase 2: + per-type dynamic-attribute fallback
# specialize— Phase 3: + compile-time CPython specialization
metaprogramming = "off"
```

- Surfaced through the existing config plumbing (mirror how `[strict]` flags are
  read). Default `"off"` so nothing changes for current users.
- Each level is a strict superset of the previous.
- **Deliverable:** the enum + parsing + threading it into the frontend lowering
  context; a classifier helper `fn metaprogramming_level(&self) -> MetaLevel`.

### Phase 0 also: the classifier

Before lowering a class/function, classify its decorators/metaclass/body into
{Tier1-pattern, Tier2-dynamic, Tier3-reject, plain}. Centralize this so the
scattered `decorator_simple_name` matches in `class.rs`/`function.rs`/
`class_init.rs` route through one table instead of growing ad-hoc arms.

- **New module:** `crates/smelt-frontend-py/src/lowering/metaprogramming.rs`
  with `enum RecognizedDecorator { Dataclass, Property, StaticMethod, ClassMethod,
  AbstractMethod, CachedProperty, Enum, NamedTuple, LruCache, TotalOrdering, … }`
  and `fn classify_decorator(&Decorator) -> Option<RecognizedDecorator>`.
- Replace the inline matches in `class.rs`/`function.rs` with calls into this
  registry. **This refactor is worth doing even at `metaprogramming = "off"`** —
  it consolidates today's scattered handling.

---

## 3. Phase 1 — pattern registry (`metaprogramming = "patterns"`)

**Goal:** hand-written concrete lowering for a fixed set of well-known,
statically-resolvable decorators/metaclasses. This is what mypy/ty do with
"plugins"; we already do it for `@dataclass`.

**Mechanism:** each recognized pattern gets a lowering rule that rewrites the
class/function HIR to its materialized form:

| Pattern | Lowering |
| --- | --- |
| `@property` / `@x.setter` | emit getter/setter methods + a backing field; reads/writes of `obj.x` route through them |
| `@staticmethod` / `@classmethod` | drop the implicit `self`/`cls` binding mode; adjust call lowering |
| `@functools.cached_property` | backing `Option<T>` field + memoized getter |
| `@functools.lru_cache` | wrap the function in a generated memoization shim keyed on args |
| `Enum` / `IntEnum` (base class) | lower to a Rust enum + value table; member access is a constant |
| `NamedTuple` / `typing.NamedTuple` | lower like a frozen dataclass/tuple struct |
| `@abc.abstractmethod` + `ABC` base | mark the method abstract (HIR already models abstract methods) |
| `@functools.total_ordering` | synthesize the missing comparison methods from the defined one |

**Files:** the registry in `lowering/metaprogramming.rs`; per-pattern lowering in
`lowering/class.rs` / `lowering/function.rs`; codegen support where a pattern
needs runtime help (e.g. lru_cache shim) in `crates/smelt-codegen-rust`.

**Pros:** precise, zero/low runtime cost, no Python toolchain needed.
**Cons:** O(1 unit of work) *per pattern*; only the whitelist; brittle to API
variants (`@dataclass(slots=True)`, decorator factories, re-exports).

**Validation:** add focused fixtures under `crates/smelt-frontend-py/tests`, one
per pattern, plus check the Python `library-probes` (`returns`, `result`,
`more-itertools`, `funcy`, `toolz`) for newly-unblocked code.

---

## 4. Phase 2 — dynamic-attribute fallback (`metaprogramming = "dynamic"`)

**Goal:** support Tier 2 — objects whose *attribute set* is dynamic
(`__getattr__`/`__setattr__`, `getattr(obj, runtime_str)`,
`setattr(obj, runtime_str, v)`, monkeypatching).

**Key asset: the machinery already exists.** The Rust runtime already has
`SmeltUnknown::Object` — an `Rc<RefCell<HashMap<String, SmeltUnknown>>>` with
identity (this is what the recent `constant`/object-identity work used). A
dynamic Python object maps directly onto it.

**Mechanism (opt-in per type, to bound blast radius):**

1. A class is **dynamic** if it defines `__getattr__`/`__setattr__`/
   `__getattribute__`, or is explicitly annotated/configured dynamic.
2. Dynamic instances are represented with a hybrid: the statically-known fields
   *plus* an overflow `SmeltUnknown::Object` attribute map.
3. `obj.attr` / `getattr(obj, k)` lowers to: try the static field; else consult
   the overflow map; else invoke `__getattr__` if defined. `setattr`/`obj.attr =`
   writes the static field if known, else the overflow map (or `__setattr__`).
4. Method dispatch on dynamic objects falls back to dynamic lookup
   (mirrors how the TS side dispatches through `SmeltUnknown`).

**Files:** representation choice in MIR (analogous to the new
`crates/smelt-mir/src/erased_record_promote.rs` pass that picks erased vs typed
records); lowering of attribute read/write + `getattr`/`setattr` builtins in
`smelt-frontend-py`; runtime helpers in `smelt-codegen-rust`.

**Pros:** handles real dynamic objects; bounded — only dynamic-marked types pay
the boxing/dispatch cost; reuses existing runtime.
**Cons:** loses static guarantees & performance for those types; `__getattribute__`
(intercepts *everything*) is especially invasive — likely keep it Tier 3.

---

## 5. Phase 3 — compile-time specialization (`metaprogramming = "specialize"`)

**The high-leverage, Python-idiomatic path.** Most Python metaprogramming runs at
**import / class-definition time** and is deterministic. Instead of
reimplementing every decorator/metaclass, **run it once at build time in real
CPython, introspect the materialized result, and lower that snapshot.**

**Mechanism:**

1. In a build pre-pass, import the target module in a real CPython interpreter
   (via PyO3, or a subprocess running a bundled `_smelt_introspect.py`).
2. For each class/function the frontend can't lower statically, **introspect the
   already-constructed object**:
   - classes: `__mro__`, `__annotations__`, materialized fields (incl. those a
     metaclass/decorator added), method names + `inspect.signature`, class-level
     constants, `__slots__`.
   - functions: resolved signature, defaults, whether a decorator replaced them.
3. Emit a **specialization manifest** (JSON) describing each materialized
   definition.
4. Feed the manifest into HIR lowering: a class with a custom metaclass becomes a
   plain class with the *materialized* field/method layout; an unknown decorator
   becomes whatever it actually produced.

This generalizes Phase 1 to **arbitrary definition-time metaprogramming** (custom
metaclasses, attrs, SQLAlchemy declarative, Pydantic v1 models, …) with **no
per-pattern code**.

**Hard requirements / caveats (be explicit):**
- **Needs the project's dependencies importable** (a real venv) — the *same*
  precondition ty/mypy have, and the exact lesson from the `httpie`/`black`
  experiment (without deps, imports erase to `Unknown` and nothing resolves).
- **Only captures structure fixed at definition time.** Per-instance dynamic
  attributes set in `__init__`/at runtime are *not* covered — those still need
  Phase 2.
- **Import side effects are a hazard.** Importing executes top-level code.
  Mitigations: run in a sandboxed subprocess, time/resource limits, and treat
  any import failure as "fall back to reject for that module."
- **Determinism must be assumed/asserted** — metaprogramming that depends on
  env/clock/network is out of scope (document + detect where cheap).
- Pairs naturally with the **ty spike** (PR #28): ty already gives inferred
  *types*; CPython introspection gives materialized *structure*. Same build-time
  Python-embedding muscle.

**Files:** a new `smelt-py-specialize` step (CLI subcommand + a Python
introspection script), the manifest schema, and a consumer in `smelt-frontend-py`
that merges materialized structure into HIR.

---

## 6. Permanently rejected (Tier 3) — keep erroring, clearly

These cannot be AOT-compiled without shipping a full interpreter (which defeats
the purpose). Keep precise diagnostics:

- `exec` / `eval` / `compile` on runtime-constructed code.
- `type(name, bases, namespace)` / `types.new_class` from runtime data.
- `importlib.import_module` / `__import__` with dynamic names; `sys.modules`
  manipulation.
- `__getattribute__` overrides (intercept *all* access).
- Runtime monkeypatching of *modules/classes you don't own* with unknown shapes.

Action: tighten/keep the existing `unsupported`/`no_metaclass` errors with a
one-line "why" and a pointer to this doc.

---

## 7. Recommended sequencing

1. **Phase 0** (flag + classifier refactor) — low risk, improves today's code
   even while `metaprogramming = "off"`. **Do first.**
2. **Phase 1** (`patterns`) — add `@property`, `@staticmethod`/`@classmethod`,
   `Enum`, `NamedTuple` next to `@dataclass`. Incremental, no new toolchain.
3. **Phase 3 spike** (`specialize`) — prototype the CPython-introspection
   manifest on one metaclass-using module and dump what Smelt would lower (same
   spirit as the ty spike). This is the one that unlocks *real-world* Python.
4. **Phase 2** (`dynamic`) — only if/when real targets need true runtime dynamic
   attributes; it's the most invasive to the static model.

Each phase ships behind the flag, defaults off, and is gated on: full
`cargo test`, the Python `library-probes` showing no regressions, and focused
frontend fixtures for every newly-supported pattern.

## 8. Risks summary

- **Phase 1:** brittle to decorator API variants; partial coverage can mislead
  (a recognized decorator with an unrecognized kwarg must error, not silently
  mis-lower).
- **Phase 2:** perf/safety cliff for dynamic types; `__getattribute__` is a trap.
- **Phase 3:** dependency + sandboxing + determinism; import side effects;
  ties build reproducibility to a Python environment.
- **Cross-cutting:** the more we materialize/erase, the more the generated Rust
  diverges from the source's apparent shape — keep diagnostics honest about what
  was specialized vs statically lowered.
