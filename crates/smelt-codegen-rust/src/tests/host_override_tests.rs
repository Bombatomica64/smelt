//! Codegen tests for host-global override-slot prelude emission.

use super::*;
use smelt_frontend_ts::{HirCtx, to_hir};
use smelt_hir::FileId;

/// Lower TypeScript to Rust with the crate-level written-host-global set seeded,
/// mirroring what the transpiler pre-pass does before lowering.
fn source_for_with_host_globals(ts: &str, names: &[&str]) -> String {
    let mut ctx = HirCtx::new();
    for name in names {
        ctx.written_host_globals.insert((*name).to_owned());
    }
    assert!(to_hir(ts, FileId(0), &mut ctx).is_ok(), "HIR");
    let mut mir = smelt_mir::lower_hir(&ctx.krate).expect("MIR lowering failed");
    smelt_mir::opt::optimize(&mut mir);
    emit_source(&mir).expect("Rust source")
}

/// A crate that reassigns a modeled host global emits the fixed override enum,
/// a per-written-name `thread_local!` slot, and the three fixed helpers, and
/// lowers the presence guard to a dynamic slot probe.
#[test]
fn slot_and_helpers_emitted_for_written_name() {
    let source = source_for_with_host_globals(
        "export function probe(): boolean { return typeof Blob !== 'undefined'; }\n\
         export function disable() { globalThis.Blob = undefined; }\n",
        &["Blob"],
    );
    assert!(
        source.contains("enum SmeltHostOverride"),
        "expected the override enum, got:\n{source}"
    );
    assert!(
        source.contains("SMELT_HOST_OVERRIDE_BLOB"),
        "expected the per-name override slot, got:\n{source}"
    );
    assert!(
        source.contains("fn smelt_host_override_present"),
        "expected the presence helper, got:\n{source}"
    );
    assert!(
        source.contains("fn smelt_host_override_write"),
        "expected the write helper, got:\n{source}"
    );
    assert!(
        source.contains("SMELT_HOST_OVERRIDE_BLOB.with(smelt_host_override_present)"),
        "presence guard must lower to a dynamic slot probe, got:\n{source}"
    );
}

/// A crate that never reassigns a host global emits none of the override
/// machinery (pay-for-use): the `typeof Blob` guard folds and native `new Blob`
/// construction is byte-identical to before.
#[test]
fn gating_off_when_no_writes() {
    let source = source_for(
        "export function probe(): boolean { return typeof Blob !== 'undefined'; }\n\
         export function make(): unknown { return new Blob(['x']); }\n",
    );
    assert!(
        !source.contains("SmeltHostOverride"),
        "no override machinery may be emitted without a write, got:\n{source}"
    );
    assert!(
        !source.contains("SMELT_HOST_OVERRIDE_"),
        "no override slot may be emitted without a write, got:\n{source}"
    );
}

/// A named class EXPRESSION whose name is a modeled host class keeps its own
/// Rust type, and the host construction it overrides keeps the host's.
///
/// `globalThis.File = class File extends Blob { .. }` is es-toolkit's
/// `isBlob`/`isFile` spec setup. Both meanings of `File` are live in that one
/// module: the emitted struct, and the native fallback the override slot runs
/// when nothing is installed. They shared one interned `Type::Class { File }`,
/// so the struct's constructor result was declared `SmeltBlob` while the blob
/// fallback was declared `File` -- seven `error[E0308]`s in the generated
/// es-toolkit crate, and nothing in the compiler able to tell them apart.
#[test]
fn a_host_named_class_expression_keeps_its_own_rust_type() {
    let source = source_for_with_host_globals(
        "export function install(): void {\n\
        \x20 globalThis.File = class File extends Blob {\n\
        \x20   name: string;\n\
        \x20   constructor(chunks: string[], filename: string) {\n\
        \x20     super(chunks);\n\
        \x20     this.name = filename;\n\
        \x20   }\n\
        \x20 };\n\
        }\n\
        export function make(): unknown {\n\
        \x20 return new File(['content'], 'example.txt');\n\
        }\n",
        &["File"],
    );
    // The class expression's own struct: declaration and constructor agree,
    // and neither is the host runtime type.
    assert!(
        source.contains("struct FileSmeltClassExpr"),
        "the class expression emits its own struct, got:\n{source}"
    );
    assert!(
        !source.contains("let _smelt_tmp_1: SmeltBlob = File::new("),
        "a struct constructor must never be declared as the host type: {source}"
    );
    // The native fallback the override slot falls back to is still the host
    // blob, because the source did not shadow the name for construction.
    assert!(
        source.contains("SmeltBlob::from_string_parts("),
        "the native fallback stays the host blob, got:\n{source}"
    );
    // Reflection still answers the SOURCE spelling. `__smelt_class` and the
    // `instanceof` class registry are compared against what the host blob's
    // own erasure stamps (`"File"`), so an internal rendering leaking into
    // either one silently deletes the `x instanceof File` branch -- es-toolkit's
    // `isFile` type-predicate spec went from passing to failing on exactly that.
    assert!(
        source.contains("smelt_register_function_classes(&smelt_erased_fn, &[\"File\"])"),
        "the class registry keeps the source spelling, got:\n{source}"
    );
    assert!(
        !source.contains("SmeltUnknown::String(\"FileSmeltClassExpr"),
        "no internal rendering may reach program output: {source}"
    );
    // Both halves of every `let` agree: no line declares one of the two types
    // and initializes it from the other.
    for line in source.lines() {
        assert!(
            !(line.contains(": SmeltBlob = FileSmeltClassExpr")
                || line.contains("FileSmeltClassExpr") && line.contains(": SmeltBlob =")),
            "declaration and initializer disagree: {line}"
        );
    }
}

/// A source class DECLARATION named after a modeled host class spells its own
/// struct on both sides of the statement.
///
/// The frontend already lets such a declaration shadow the host class (the name
/// IS in the enclosing scope, and a reference from another module must resolve
/// to the one symbol). Codegen has to answer "does the source own this name"
/// the same way, or the type resolver hands back the host runtime type for a
/// value the class item constructs.
#[test]
fn a_source_class_declaration_shadows_the_host_runtime_type() {
    let source = source_for(
        "class File {\n\
        \x20 name: string;\n\
        \x20 constructor(filename: string) {\n\
        \x20   this.name = filename;\n\
        \x20 }\n\
        }\n\
        export function make(): string {\n\
        \x20 return new File('example.txt').name;\n\
        }\n",
    );
    assert!(
        source.contains("struct File {"),
        "the source class emits its struct, got:\n{source}"
    );
    assert!(
        !source.contains("SmeltBlob"),
        "a shadowed host class must not reach the generated crate: {source}"
    );
}
