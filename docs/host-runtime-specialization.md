# Host-runtime metaprogramming specialization

Smelt uses CPython and Node only as sandboxed build-time partial evaluators.
Generated applications never link or launch either host runtime.

The compiler pipeline is:

1. resolve and strictly type-check the complete source graph;
2. conservatively detect definition-time metaprogramming and propagate
   candidacy through imports and re-exports;
3. compute a cache key over source, dependency, lockfile, environment,
   runtime, compiler, adapter, policy, and callable-provenance identities;
4. run one hard-sandboxed host guest per language and cache key on a miss;
5. validate the versioned specialization manifest against static types;
6. lower source plus materialized definitions into ordinary typed HIR;
7. continue through MIR and Rust code generation.

Frontend parsing and lowering remain pure. A module that needs specialization
must receive a manifest through an option-aware entry point; frontends never
start host processes. Missing materialization is diagnosed as
`smelt::specialization-required`.

## Static boundary

The manifest owns final definition bindings, concrete class layout, MRO,
fields, descriptors, methods, slots, static values, metadata, constructor
shape, signatures, defaults, annotations, wrapper chains, initializers, and
deterministic observable effects. Values form an ID-addressed graph so cycles,
shared identity, callable references, and typed instances survive
serialization.

Every runtime callable must map to a source function, nested function, lambda,
or method. Its provenance contains a source span and code hash, concrete
closure captures, and receiver mode. Frontends lift these callables into
hidden HIR functions with explicit environments. Native callables, generated
bytecode, and unserializable state require a versioned native specialization
adapter and otherwise produce a precise diagnostic.

Ordinary values retain concrete static types. `DynamicMetadata` is restricted
to intentional dynamic metadata or source-level `unknown` boundaries.

## Cache and package artifacts

Manifest builds resolve the complete source graph and run the centralized
detector before frontend lowering. Candidate Python modules share one CPython
batch; candidate TypeScript modules share one canonical `tsc` emission and one
Node batch. Unaffected builds do not inspect or launch either guest.

Each language batch is addressed by a cache key covering compiler/schema and
host-toolchain versions, source and dependency identities, lockfiles,
allowlisted environment values, sandbox policy, callable provenance, and
native adapter versions. Smelt checks the project cache first and then the
generated package artifact. Only a miss starts the sandboxed guest.

Artifacts use the same validated JSON envelope in both locations:

```text
.smelt/specialization-cache/{python,typescript}/<cache-key>.json
<output>/.smelt/specialization/{python,typescript}/<cache-key>.json
```

The output form can be transported with a package and loaded through the same
artifact-store interface. Identical bytes are never rewritten, preserving
mtime-based incremental builds. The writable scratch root is recorded as the
stable `<scratch>` policy identity rather than a process-specific temporary
path.

## Sandbox contract

Specialization fails closed when no supported hard sandbox is available.
Network, child processes, uncontrolled native extensions, and writes outside
the scratch directory are denied by default. Source, dependencies, and runtime
files are read-only. The environment is sanitized and allowlisted. Wall time,
CPU time, memory, process count, and output are bounded. The exact effective
policy is embedded in both the manifest and cache key.

## Unsupported effects

External filesystem changes, network operations, uncontrolled subprocesses,
time/random-dependent results, and opaque native side effects are rejected or
delegated to explicit adapters. Deterministic global/static mutations, output,
and source-mappable initialization can be baked into final state or replayed
in source order during generated module initialization.
