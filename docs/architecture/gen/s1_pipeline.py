"""Section 1: the pipeline."""
from svg import Svg, L, H, S


def build():
    W = 1040
    g = Svg(W, 1050, "Smelt pipeline from sources to the generated crate")
    SX, SW = 16, 600          # spine
    RX, RW = 660, 364         # side column

    def down(a, b, label, x=None, lw=None):
        x = x if x is not None else a["cx"]
        g.path(f"M{x:.0f},{a['b']} V{b['y'] - 2}")
        if label:
            g.tag(x, (a["b"] + b["y"]) / 2, label, kind="label")

    tsr = "crates/smelt-transpiler/src/"
    a = g.node(SX, 16, SW, 78, [
        H("Manifest and sources  ·  smelt-transpiler"),
        L("lowering::ManifestLowering::for_manifest(config, path)"),
        L("Smelt.toml → manifest::dependency_closure → dependency order"),
    ], kind="driver", href=tsr + "lowering.rs")

    b = g.node(SX, 142, SW, 98, [
        H("Type gate  ·  ManifestLowering::check"),
        L("ts_facts::analyze_graph → TsCheck { facts: Option<TsTypeFacts> }"),
        L("python::check_sources → ty diagnostics"),
        L("→ Checked<ReadyLowering>; into_inner(TypeErrorPolicy)"),
    ], kind="check", href=tsr + "type_gate.rs")
    down(a, b, "Vec<ManifestSource>")

    # frontends container
    cy = 288
    c = g.node(SX, cy, SW, 198, [
        H("Frontends  ·  lowering::driver::PredeclaredLowering::lower_source"),
        L("one HirCtx shared by every file; dispatch on SourceLang"),
    ], kind="front", href=tsr + "lowering/driver.rs")
    ts = g.node(SX + 14, cy + 66, 282, 118, [
        L("smelt-frontend-ts", bold=True),
        L("to_hir_with_options(..)"),
        L("oxc → ModuleBuilder::program"),
        L("FrontendOptions { ts_facts,"),
        L("  specialization, trust, .. }"),
    ], kind="front", href="crates/smelt-frontend-ts/src/lowering.rs", cls="node inner")
    py = g.node(SX + 304, cy + 66, 282, 118, [
        L("smelt-frontend-py", bold=True),
        L("to_hir_with_options(..)"),
        L("ruff_python_parser → HIR"),
        L("ty_resolve::ResolvedTypes"),
        L("  (wraps smelt-py-types)"),
    ], kind="front", href="crates/smelt-frontend-py/src/lib.rs", cls="node inner")
    down(b, c, "ReadyLowering + &TsTypeFacts")

    d = g.node(SX, 556, SW, 78, [
        H("HIR  ·  smelt_hir::Crate"),
        L("modules, items, bodies, types: TypeInterner, symbols"),
        L("erasures: Vec<ErasureRecord>   (frontend decisions)"),
    ], kind="hir", href="crates/smelt-hir/src/krate.rs")
    down(c, d, "LoweredCrate { krate, modules, sources }")

    e = g.node(SX, 688, SW, 78, [
        H("MIR  ·  smelt_mir::compile_observed"),
        L("lower_hir → opt::try_optimize → validate"),
        L("CFG of BasicBlocks over typed locals; types cloned from HIR"),
    ], kind="mir", href="crates/smelt-mir/src/compile.rs")
    down(d, e, "&smelt_hir::Crate")

    f = g.node(SX, 820, SW, 98, [
        H("Codegen  ·  emit_crate_with_modules_reporting"),
        L("module_map::emit_mapped_sources → crate_builder::emit_root"),
        L("EmitContext::new (RuntimeDemand::of) → CrateBuilder::with_prelude"),
        L("→ emit_free_functions → emit_class_impls → emit_bound_traits"),
    ], kind="codegen", href="crates/smelt-codegen-rust/src/crate_writer.rs")
    down(e, f, "&smelt_mir::Mir  (+ &Crate, modules)")

    o = g.node(SX, 966, SW, 78, [
        H("Generated crate  ·  <output>/"),
        L("Cargo.toml, src/main.rs | lib.rs, src/<module>.rs (write_if_changed)"),
        L("smelt-erasure.json, smelt-typedness.json (pipeline.rs)"),
    ], kind="driver", href=tsr + "pipeline.rs")
    down(f, o, "files + Vec<ErasureRecord> (backend)")

    # side column
    t7 = g.node(RX, 16, RW, 116, [
        H("smelt-ts-types  (feature ts7)"),
        L("analyze_program(CheckProgram, ..)"),
        L("embeds tsrs, a Rust port of the"),
        L("TypeScript 7.1 checker (git dep)"),
        L("→ TsTypeFacts: span → TsType"),
    ], kind="check", href="crates/smelt-ts-types/src/lib.rs")
    pyt = g.node(RX, 152, RW, 96, [
        H("smelt-py-types  (feature ty)"),
        L("check_sources: ty diagnostics"),
        L("resolve_module_types →"),
        L("  ResolvedModuleTypes"),
    ], kind="check", href="crates/smelt-py-types/src/lib.rs")
    g.path(f"M{RX},{t7['cy']:.0f} H{SX + SW + 22} V{b['y'] + 30} H{SX + SW + 2}")
    g.path(f"M{RX},{pyt['y'] + 30} H{SX + SW + 2}")
    g.path(f"M{RX + 30},{pyt['b']} V{cy + 20} H{SX + SW + 2}")
    g.tag(RX + 40, cy + 2, "ResolvedModuleTypes", kind="label", anchor="start")

    sp = g.node(RX, 330, RW, 96, [
        H("smelt-specialize"),
        L("specialization::PreparedSpecialization"),
        L("runs host runtimes in a sandbox"),
        L("→ SpecializationManifest"),
    ], kind="support", href="crates/smelt-specialize/src/lib.rs")
    g.path(f"M{RX},{sp['cy']:.0f} H{SX + SW + 2}")

    sl = g.node(RX, 452, RW, 116, [
        H("smelt-stdlib  (metadata only)"),
        L("typescript_call_rule → RuleId"),
        L("  asked by both frontends"),
        L("StdlibClass, runtime_symbols"),
        L("  read by HIR, MIR, codegen"),
    ], kind="support", href="crates/smelt-stdlib/src/lib.rs")
    g.path(f"M{RX},{sl['y'] + 26} H{SX + SW + 2}", cls="edge light")
    g.path(f"M{RX},{sl['b'] - 14} H{SX + SW + 40} V{f['y'] + 20} H{SX + SW + 2}", cls="edge light")

    rt = g.node(RX, 786, RW, 136, [
        H("smelt-runtime  (text, not a dependency)"),
        L("compiled, unit-tested Rust with"),
        L("// @smelt:item regions"),
        L("order::ORDER + source::render_item"),
        L("→ runtime_prelude::"),
        L("  emit_runtime_prelude  → codegen"),
    ], kind="runtime", href="crates/smelt-runtime/src/lib.rs")
    g.path(f"M{RX},{f['y'] + 60} H{SX + SW + 2}")

    note = g.node(RX, 966, RW, 78, [
        S("Generated crates inline the runtime text;"),
        S("their Cargo.toml never names smelt-runtime."),
        S("Boxes link to the file that holds the call."),
    ], kind="note", cls="node note")
    return g.render()
