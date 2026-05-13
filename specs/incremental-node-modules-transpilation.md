# Incremental Node Modules Transpilation

## Goal

After v1, Smelt should support dependency packages by transpiling their source once, usually in CI by the package creator or by a shared package cache builder, and then reusing those native Rust artifacts in downstream projects.

The default model is not "skip `node_modules`". The model is:

- package maintainers or cache infrastructure transpile package source into reusable Smelt artifacts;
- downstream applications consume those artifacts quickly;
- if no package-maintainer artifact exists, Smelt performs the transpilation work itself from package source;
- when the application has stronger concrete types than the package artifact had, Smelt incrementally recompiles only the affected generic or unknown-dependent units;
- generated Rust stays fast at runtime, even if the CI/cache step does more work.

## Motivation

Real TypeScript packages often use broad public types internally, such as:

```ts
type StrictFunction = (...args: never) => unknown;
```

That type is intentionally generic and type-level. A package-local artifact may need to compile it as an opaque callable boundary. But an application that calls into the package may know the concrete function type and argument types. Smelt should be able to use those stronger downstream types to produce better Rust without retranspiling the entire package graph from scratch.

This is especially important for libraries like Remeda, Effect, date-fns, and similar utility packages where generic public APIs hide many concrete call shapes used by the app.

## Non-Goals

- Do not compile all installed packages from source on every app build.
- Do not rely on JavaScript runtime execution for package internals that Smelt has already transpiled.
- Do not make compile-time speed more important than generated Rust runtime quality.
- Do not erase all dependency APIs to `unknown` just to keep builds fast.
- Do not require every package to be recompiled with every downstream project if no downstream type information changes its generated code.

## Artifact Model

A Smelt dependency artifact should contain:

- generated Rust code for the package implementation;
- generated `.d.ts` files for every exported TypeScript-facing surface;
- generated `.pyi` files for every exported Python-facing surface;
- normalized package metadata:
  - package name;
  - version;
  - lockfile identity;
  - package manager identity if relevant;
  - source entrypoints;
  - relevant `tsconfig` options;
- exported type surface:
  - functions;
  - overloads;
  - classes;
  - interfaces;
  - type aliases;
  - exported constants;
  - module namespaces;
- lowered HIR/MIR for package implementation units;
- dependency edges between package modules;
- a specialization index for generic and unknown-dependent functions;
- stable hashes for every unit that can be reused independently;
- generated Rust source or a compact MIR artifact ready for Rust codegen;
- diagnostics explaining which parts were fully native, summarized, deferred, or rejected.

The artifact is therefore both a native implementation package and a type-surface package. Rust code is what downstream Smelt builds link or specialize. `.d.ts` and `.pyi` files are what downstream source frontends use to understand the package boundary without reparsing or retranspiling the whole package source.

Every artifact should have a layout similar to:

```text
smelt-artifact/
  SmeltArtifact.toml
  rust/
    Cargo.toml
    src/
      lib.rs
      ...
  typescript/
    index.d.ts
    ...
  python/
    package_name.pyi
    ...
  mir/
    units/
      ...
  diagnostics/
    build.json
```

Artifacts should be content-addressed. The key should include:

- Smelt version;
- package source hash;
- package version;
- relevant compiler options;
- enabled Smelt feature flags;
- dependency artifact hashes;
- public type surface hash.

## Compilation Modes

### Package CI Mode

Package CI mode is run by the package creator or cache builder.

It should:

- transpile the package source;
- run package tests when supported;
- produce a reusable artifact;
- publish or cache that artifact;
- record unresolved dynamic points as explicit specialization hooks.

This mode can be slower because it is not on every downstream app build.

### App Consumption Mode

App consumption mode is run by the downstream project.

It should:

- load package artifacts from cache;
- use package type surfaces for checking application code;
- link pretranspiled package Rust when no specialization is needed;
- request incremental specialization when application types make a package unit more concrete;
- fail clearly when a required package artifact is missing and source transpilation is disabled.

### Fallback Source Mode

Fallback source mode transpiles package source locally when no artifact exists.

This is required for correctness: package maintainers publishing Smelt artifacts is an optimization, not a prerequisite for compatibility. If a dependency has no artifact, Smelt must be able to do the work itself.

Fallback source mode should:

- discover the package source from `node_modules` and package metadata;
- transpile the package into the same artifact format a maintainer would have published;
- cache the resulting artifact by package source hash and Smelt version;
- allow CI to upload or persist the generated artifact for future builds;
- produce the same Rust, `.d.ts`, and `.pyi` outputs as package CI mode.

It should still avoid silently recompiling the same package on every app build. The expensive fallback step should happen once per cache key, then downstream builds should behave like artifact consumption mode.

## Incremental Specialization

Some dependency units should compile once in a generic or opaque form, then specialize when downstream types are known.

Examples:

- `unknown` narrowed by application code;
- generic function calls with concrete `T`;
- overload selection at a downstream call site;
- `(...args: never) => unknown` used as a broad callable boundary;
- callback-heavy APIs where the app supplies concrete callback types;
- object or record APIs where the app passes concrete key/value shapes.

Specialization should work at function or module-unit granularity. It should not require recompiling the whole package.

The specialization key should include:

- artifact unit hash;
- selected overload or call signature;
- concrete type substitutions;
- relevant narrowed unknown shapes;
- relevant const generic or literal information if Smelt later supports it.

## Handling `unknown`

Dependency artifacts may contain `unknown` where the package genuinely has no local knowledge.

Downstream builds should be able to refine those `unknown` values when the app narrows or asserts them. If the value remains unknown at the app boundary, generated Rust must keep a safe tagged `SmeltUnknown` representation.

The preferred order is:

1. use concrete package-local types;
2. specialize from downstream concrete types;
3. use safe tagged unknown;
4. reject only when runtime behavior would be unsound or impossible to represent.

## Handling `never`

`never` must distinguish value-bearing positions from type-level positions.

Value-bearing `never` remains impossible to materialize.

Type-level callable patterns such as:

```ts
type StrictFunction = (...args: never) => unknown;
```

can appear in package artifacts as opaque callable boundaries. Downstream specialization may replace them with concrete callable shapes when the app provides enough type information.

## Runtime Quality

Runtime speed has priority over minimizing the initial package CI transpilation cost.

That means:

- prefer specialized Rust for hot concrete app call sites;
- avoid routing everything through tagged dynamic values;
- avoid large dynamic dispatch layers when concrete types are known;
- allow package CI/cache builds to do heavier analysis;
- cache aggressively so downstream app builds stay practical.

Tagged runtime values are still allowed where the source package truly crosses dynamic boundaries, but they should not become the default representation for all dependency code.

## Cache Invalidation

A cached artifact is invalid when any of these change:

- package source;
- package version;
- package public type surface;
- package compiler settings that affect emitted behavior;
- Smelt frontend, HIR, MIR, or codegen version;
- stdlib/runtime mapping version;
- enabled compatibility flags;
- dependency artifact hash.

Specialized units are invalid when:

- their base artifact unit changes;
- the downstream concrete type substitution changes;
- the selected overload changes;
- unknown narrowing facts change;
- relevant app-side callback or type alias shapes change.

## Developer Workflow

Package creator:

```bash
smelt package build
smelt package test
smelt package publish-artifact
```

Application developer:

```bash
smelt build
```

If an artifact is missing:

```text
missing Smelt artifact for remeda@x.y.z
hint: run `smelt package build remeda` or enable fallback source mode
```

If specialization is needed:

```text
specializing remeda:purry for concrete call shape hash ...
```

## Configuration Sketch

```toml
[node_modules]
mode = "artifact-first"
fallback-source = false

[node_modules.cache]
path = ".smelt/cache/node-modules"
allow-global-cache = true

[node_modules.specialization]
enabled = true
max-units-per-package = 256

[node_modules.packages.remeda]
artifact = "required"
specialize = true
```

Modes:

- `artifact-first`: use artifacts and specialize when needed;
- `source-first`: transpile local package source and populate cache;
- `artifact-only`: fail if no artifact exists; useful only for locked-down CI;
- `source-disabled`: use only app code and explicit native externs.

Default post-v1 behavior should be `artifact-first` with fallback source enabled in developer and CI environments. Package-maintainer artifacts make builds faster, but absence of those artifacts should not prevent Smelt from compiling a compatible package when source is available.

## Acceptance Criteria

Initial post-v1 slice:

- Smelt can build and reuse a cached artifact for one TypeScript package.
- Downstream app builds do not retranspile unchanged package source.
- The artifact records public type surfaces and implementation unit hashes.
- A downstream app can trigger one function-level specialization from concrete types.
- The specialization cache avoids recompiling the same concrete shape twice.
- Diagnostics show whether code came from app source, package artifact, fallback source, or specialization.

Remeda-oriented slice:

- A package artifact can represent `StrictFunction = (...args: never) => unknown`.
- A downstream call can specialize the relevant callable shape when concrete callback and argument types are known.
- The package artifact can compile broad internal helpers without forcing all call sites through tagged `unknown`.

## Open Questions

- Artifact distribution format: crate, binary blob, JSON plus MIR, or mixed layout?
- How much Rust source should package artifacts store versus MIR-only data?
- How should artifact trust and provenance work for CI-published package artifacts?
- Can package artifacts be shared across package managers without losing lockfile correctness?
- What is the right cap for specialization explosion in large apps?
- Should hot-path specialization be user-directed, profile-directed, or purely type-driven?
