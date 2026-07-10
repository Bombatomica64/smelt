# date-fns full-corpus baseline (2026-07-10)

## Reproduction

- Source: `date-fns/date-fns` at `09ece22eaea4ffab1a8fa396beeeb6a451dcfbf4`
  (the latest commit available at the documented May 12, 2026 probe date).
- Corpus: `src/**/*.ts` and `src/**/*.tsx`, excluding declaration files through
  Smelt's normal manifest discovery.
- Build manifest: local ignored probe manifest with `src/index.ts` as the entry
  and the full source tree as test globs.
- Command: `smelt rust-test-report --build-manifest <manifest>
  --cargo-manifest <dist>/Cargo.toml --full --diagnostics --suppress-warnings`.

## First blocker

The report command stops during source lowering before a generated Cargo
manifest exists:

```text
src/_lib/defaultOptions/index.ts
module-level mutable binding initializer must be a literal for now
```

The source shape is a module-level mutable, concretely typed empty options
record:

```ts
let defaultOptions: DefaultOptions = {};
```

An isolated type probe shows `DefaultOptions` lowers to
`Dict<String, Unknown>`; the value is not a TypeScript `unknown` boundary.
Current mutable-global HIR and MIR items carry only a primitive `Literal` /
`Constant` initializer, and Rust codegen only emits `Cell` for numeric/bool
values or `RefCell<String>` for strings. The newer module-global lift therefore
rejects this previously reached date-fns surface before Rust emission.

## Required general fix

Extend mutable globals with an explicit typed default/empty-collection
initializer through HIR and MIR, and emit non-copy concrete global types through
`RefCell<T>`. This must preserve the concrete dictionary shape; routing the
record through `SmeltUnknown` would violate the repository's static-shape
boundary rules. No `SmeltUnknown` usage was added during this investigation.

After that frontend gate is cleared, rerun `smelt rust-test-report` to obtain the
generated Rust diagnostic families. The older May baseline expected the next
tail to include optional-callable representation and Date-setter numeric casts,
but those assumptions must be remeasured on current main.
