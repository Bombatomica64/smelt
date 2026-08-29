//! Runtime helpers for transpiled code — the *source of truth* for the runtime
//! prelude that `smelt-codegen-rust` inlines into every generated crate.
//!
//! # Why the helpers live here
//!
//! Historically every runtime helper existed only as string literals inside
//! `smelt-codegen-rust` (`writer.line("fn smelt_next_object_id() -> usize {")`
//! and ~1,700 siblings). Runtime code in that form is never type-checked,
//! clippy'd or unit-tested on its own: the only way to learn whether a ten-line
//! helper was correct was to emit a whole crate and `cargo test` it, which costs
//! tens of seconds per case.
//!
//! The helpers in this crate are ordinary, compiled, unit-tested Rust. Each one
//! is wrapped in `// @smelt:item <name>` / `// @smelt:item-end` markers, and
//! [`source::item_source`] hands the emitter the marked text verbatim. The
//! emitter selects items by name through the gate manifest in
//! `smelt-codegen-rust`'s `runtime_prelude` module, so the *selection* logic
//! (the `needs_*` predicates computed from MIR) is unchanged — only the source
//! form moved.
//!
//! # What generated crates depend on
//!
//! Nothing. Generated crates stay self-contained: the selected items are
//! emitted inline into the generated crate root, exactly as before, and no
//! generated `Cargo.toml` gains a `smelt-runtime` dependency. Making this a
//! real dependency needs a stable generated ABI (`SmeltList` reference
//! semantics and byte-buffer views are still in flight), so it is deliberately
//! out of scope.
//!
//! # Invariants for anyone editing these modules
//!
//! * The marked text is emitted **verbatim**, so editing it changes the bytes of
//!   every generated crate. Formatting inside a marked region is part of the
//!   generated output.
//! * Marked regions are emitted into a crate root that already carries
//!   `#![allow(dead_code, non_snake_case, unused_imports, unused_variables)]`,
//!   and where every runtime item shares one flat module. Items may therefore
//!   only reference other runtime items and `::std` (plus `::serde_json` /
//!   `::regex` / `::tokio` where the corresponding backend dependency gate is
//!   also on) — never `crate::`, `super::` or a helper defined outside a marked
//!   region.
//! * Items with a dependency on another item are placed so that the dependency
//!   is a crate-root or ancestor-module item here (private items are visible to
//!   descendant modules), because the emitted form has no `use` statements to
//!   carry.
//! * Every item named by the emitter's manifest must exist; the manifest test
//!   `runtime_manifest_items_resolve` in `smelt-codegen-rust` fails loudly with
//!   the missing names when it does not.

pub mod captures;
pub mod clock;
pub mod source;
pub mod strings;
pub mod uri;
pub mod value;
