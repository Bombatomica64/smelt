# Metaprogramming support

Smelt lowers a strictly-typed, statically-meaningful subset of TypeScript and
Python to Rust. Most metaprogramming in real code happens **once, at import /
definition time**, to produce a stable class or binding shape. Smelt handles
that class of metaprogramming through **host-runtime specialization**: it runs
the source module once in a hard sandbox using real CPython / Node *only as a
build-time partial evaluator*, snapshots the resulting static shape into a
versioned manifest, and lowers that manifest into ordinary typed HIR. Generated
applications never link or launch a host runtime.

See [`host-runtime-specialization.md`](host-runtime-specialization.md) for the
mechanism. This document is the practical boundary: what specializes, what is
rejected, and — importantly — what must fail *loud* rather than silently
producing an incomplete shape.

## The actual boundary

The dividing line is **not** "everything except `eval`." A construct
specializes only if it is all three of:

1. **Import-time stable** — the shape is fixed when the module finishes
   importing and does not change per runtime value or per instance.
2. **Deterministic** — it does not depend on the clock, RNG, network,
   subprocesses, filesystem, or other ambient input during definition.
3. **Source-traceable** — every resulting callable maps back to a concrete
   source function, lambda, or method (provenance: source span + code hash +
   closure captures + receiver mode).

Violate any one and the construct falls outside specialization. `eval`/`exec`
are just the most obvious members of the excluded set.

## Supported (specializes today)

These run at import time, produce a stable shape, and have source-traceable
callables:

| Construct | Notes |
| --- | --- |
| Metaclasses that fix a layout at class-creation time | Incl. `ABCMeta`; Django `ModelBase` when a manifest is present |
| Class decorators | Including `@dataclass` and decorator factories |
| Function decorators / `functools.wraps` wrapper chains | Wrapper chain is materialized; each layer maps to source |
| Descriptors (data and non-data) | `__get__`/`__set__` layout captured into fields/methods |
| `dataclasses` | Fields, defaults, generated `__init__` shape |
| `__slots__` | Slot layout captured |
| Enums | Members and values |
| Named tuples / attrs-style generated classes | Generated fields/methods captured |
| Deterministic module-init side effects | Baked into final state or replayed in source order during generated module init |
| Closures / nested functions / lambdas | Captured with explicit environments and concrete captures |

The manifest is treated as **authoritative** for final bindings, MRO, fields,
descriptors, methods, slots, static values, metadata, constructor shape,
signatures, defaults, and annotations — because it is derived from the real
interpreter, it is ground truth, not a heuristic.

## Must fail loud (currently a silent gap — see below)

These produce a *runtime-dynamic* surface that the import-time snapshot cannot
represent. The correct behavior is a hard, source-located diagnostic
(`smelt::specialization-required` / `smelt::dynamic-attribute-access`) — **not**
a partial shape:

| Construct | Why it can't specialize |
| --- | --- |
| `__getattr__` / `__getattribute__` override | Attributes resolve per-call from runtime state; no fixed field set |
| `__setattr__` / `__delattr__` override | Attribute *set* is open and runtime-determined |
| JS `Proxy` traps / computed `Object.defineProperty` getters | Same: property surface is dynamic |
| Instance attributes never stored in `__dict__`/`__slots__` | Snapshot reads only `__dict__`/`__slots__`, so these are invisible |

> **Status:** as of the specialization work in progress, the guest reads only
> `__dict__` and `__slots__` and does **not** detect these overrides, so such
> objects are captured as their static shell with no diagnostic. This is the
> highest-priority correctness gap: silent under-capture lowers cleanly and then
> fails (or misbehaves) far downstream. Detection of these overrides on a class
> or its MRO should emit a hard diagnostic with the source span.

## Unsupported / rejected (by design)

These are rejected outright, or require an explicit, versioned native
specialization adapter:

| Construct | Disposition |
| --- | --- |
| `eval` / `exec` / `compile`-built callables | Rejected — non-source-traceable code |
| Runtime-synthesized bytecode, C-level callables with no source | Native adapter required, else diagnostic |
| Post-import mutation / monkeypatching (`setattr` on a class, `obj.__class__ = X`, gevent-style patching) | Not represented — snapshot is taken once |
| Class shape that branches on env var / feature flag / installed-plugin set / platform | Snapshot reflects only the probe environment; cache key includes environment identity so stale snapshots are not silently reused, but input-dependent shape cannot be represented |
| Nondeterministic definition-time effects (clock, RNG, network, subprocess, filesystem at import) | Rejected by the sandbox; delegate to an adapter if genuinely needed |
| `any` / source-level `unknown` data flow routed as normal values | Use a real dynamic boundary (`DynamicMetadata` / `SmeltUnknown`) intentionally, not as a default ABI |

## Rule of thumb

If a framework uses metaprogramming to *define* a class and then the class
behaves like an ordinary class at runtime, Smelt can specialize it. If the
object keeps doing metaprogramming *at runtime* — synthesizing attributes,
intercepting access, mutating itself, or depending on runtime input — it is out
of scope, and Smelt should tell you so explicitly rather than guess.
