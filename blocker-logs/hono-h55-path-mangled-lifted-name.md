# H55 — a lifted module-level arrow's Rust name embeds the source path

Found while writing H50's fixture (round 24). **Not fixed**, and pre-existing —
H50 only made the shape easy to reach.

## What happens

Referencing a module-level `const` arrow as a VALUE lifts it to a named Rust
function, and the name carries the source path it was compiled from:

```
is_short__module__tmp_claude_0_..._scratchpad_cba_src_main_ts
```

Built from a different directory, the same TypeScript emits a different Rust
name. Two consequences:

1. **A generated crate's API depends on where it was built.** The same source
   compiled in CI, in a container, and on a laptop produces three different
   function names in `src/main.rs`. Nothing downstream can name one, and any
   diff between two builds of the same source is noise.
2. **No golden fixture can cover the shape.** The end-to-end harness copies
   `input.ts` into a fresh temp project, so `expected.rs` would hold a
   `/tmp/.../<random>` path. That is why H50's regression test is a
   build-and-run test in
   `crates/smelt-transpiler/tests/hir_cli_cross_language_tests.rs` rather than
   an `examples/` fixture, and the test says so at the top.

The mangling itself is doing a real job — the lifted name has to be unique
across modules, exactly like the class-name collision H51 just solved — so the
fix is to derive it from the same thing H51 derives from: the module's identity
within the crate (its manifest-relative path or its module ordinal), not the
absolute path the compiler happened to be handed. `manifest_module_name` /
`manifest_module_names` in `crates/smelt-transpiler/src/lowering.rs` already
compute exactly that, and the older note
`blocker-logs/module-arrow-lifted-capture.md` shows the name as
`second__module_src/main.ts` — a manifest-RELATIVE path — so the absolute
spelling may be a regression rather than the original design.

## Cost of leaving it

Only cosmetic for a program that runs, but it blocks golden coverage of every
lifted-arrow shape, and it makes generated output non-reproducible, which is a
property worth having for a transpiler whose output is committed.
