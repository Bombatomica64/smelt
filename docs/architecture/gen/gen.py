"""Assemble docs/architecture/index.html."""
import json, subprocess, sys, os, html
sys.path.insert(0, os.path.dirname(__file__))
import svg as svgmod
from svg import esc, link
import s1_pipeline, s2_crates, s3_modules, s4_types, s5_mir, s6_gates, s8_heat

ROOT = __import__("subprocess").check_output(["git", "rev-parse", "--show-toplevel"], cwd=__import__("os").path.dirname(__import__("os").path.abspath(__file__))).decode().strip()
OUT = ROOT + "/docs/architecture/index.html"
COMMIT = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip()
CDATE = subprocess.check_output(["git", "log", "-1", "--format=%cs", COMMIT], cwd=ROOT).decode().strip()


def a(path, text=None, code=True):
    t = esc(text or path)
    if code:
        t = f"<code>{t}</code>"
    return f'<a href="{esc(link(path))}">{t}</a>'


def gitdate(path):
    return subprocess.check_output(["git", "log", "-1", "--format=%cs", "--", path], cwd=ROOT).decode().strip()


churn = {r["module"]: r for r in json.load(open(os.path.dirname(__file__) + "/churn.json"))}


def commits(*mods):
    return sum(churn.get(m, {}).get("commits", 0) for m in mods)


def fig(svg_text, cls=""):
    return f'<div class="fig {cls}">{svg_text}</div>'


parts = []
P = parts.append

# ---------- 1 pipeline
s1 = s1_pipeline.build()

# ---------- 2 crates
g2, edges, dev, lines = s2_crates.render()
s2 = g2.render()
member_lines = {c: v for c, v in lines.items() if c != "smelt-gui"}
total_src = sum(s for s, o in member_lines.values())

# ---------- 3 modules
allf, prodf, nfiles, nlines = s3_modules.top_files()
maps = {}
for c in ["smelt-frontend-ts", "smelt-codegen-rust", "smelt-frontend-py", "smelt-mir", "smelt-runtime", "smelt-transpiler"]:
    maps[c] = s3_modules.build(c, prodf)

# ---------- 4
s4, summ = s4_types.build()
# ---------- 5
s5a, s5b = s5_mir.model(), s5_mir.passes()
# ---------- 6
s6 = s6_gates.build()
# ---------- 8
s8, pts = s8_heat.build()

with open(os.path.dirname(__file__) + "/style.css") as f:
    CSS = f.read()
with open(os.path.dirname(__file__) + "/script.js") as f:
    JS = f.read()

LEGEND = [("driver", "smelt-transpiler, files on disk"), ("check", "type facts: tsrs, ty"), ("front", "frontends"),
          ("hir", "HIR"), ("mir", "MIR"), ("codegen", "Rust codegen"), ("runtime", "runtime text"),
          ("support", "stdlib, specialize"), ("loss", "erasure to SmeltUnknown"), ("good", "type recovered")]

TOC = [("pipeline", "1. The pipeline"), ("crates", "2. Crate graph"), ("modules", "3. Inside the big crates"),
       ("types", "4. Life of a type"), ("mir", "5. MIR in one picture"), ("gates", "6. Quality gates"),
       ("lookup", "7. Where does X live?"), ("heat", "8. Heat map"), ("notes", "Notes")]

P(f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Smelt architecture map</title>
<meta name="description" content="Visual map of the Smelt transpiler derived from the code at {COMMIT[:9]}.">
<style>{CSS}</style>
</head>
<body>
<svg class="defs" aria-hidden="true" width="0" height="0"><defs>
<marker id="arr" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" class="arrowhead"/></marker>
<marker id="arr-loss" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" class="arrowhead-loss"/></marker>
<marker id="arr-good" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" class="arrowhead-good"/></marker>
</defs></svg>
<header class="top">
  <div class="title">
    <h1>Smelt architecture map</h1>
    <p class="meta">Derived from the code at <code>{COMMIT[:9]}</code> ({CDATE}). Every box links to the file it names. Regenerate with the commands in {a("docs/architecture/README.md", "README.md")}.</p>
  </div>
  <button id="theme" type="button" aria-label="Toggle light and dark theme">Theme: auto</button>
</header>
<nav class="toc" aria-label="Contents"><ul>{"".join(f'<li><a href="#{i}">{esc(t)}</a></li>' for i, t in TOC)}</ul></nav>
<main>
<div class="legend" aria-label="Colour legend">{"".join(f'<span class="lg-item k-{k}"><i></i>{esc(t)}</span>' for k, t in LEGEND)}</div>
""")

# 1
P(f"""<section id="pipeline"><h2>1. The pipeline</h2>
{fig(s1)}
<p>One driver, {a("crates/smelt-transpiler/src/lowering.rs", "ManifestLowering")}, serves <code>build</code>, <code>check</code>, <code>probe</code>, the dumps and every report. The labels on the arrows are the Rust values handed over. Type facts enter in two places: the type gate runs tsrs and ty over every source before anything lowers, and the frontends read the facts back by span (<code>FrontendOptions::ts_facts</code>) or through <code>ty_resolve</code>. The frontend decides almost every erasure; MIR and codegen inherit them (section 4).</p>
</section>""")

# 2
rows = []
crates_sorted = sorted(s2_crates.POS, key=lambda c: (s2_crates.POS[c][0], s2_crates.POS[c][1]))
dep_set = {(x[0], x[1]): x for x in edges}
devset = set(dev)
head = "".join(f"<th><span>{esc(c.replace('smelt-', ''))}</span></th>" for c in crates_sorted)
for r in crates_sorted:
    cells = []
    for c in crates_sorted:
        if (r, c) in dep_set:
            e = dep_set[(r, c)]
            feat = s2_crates.FEATURE.get((r, c))
            cells.append(f'<td class="d">{"opt" if e[2] else "dep"}{(" · " + feat) if feat else ""}</td>')
        elif (r, c) in devset:
            cells.append('<td class="dv">dev</td>')
        else:
            cells.append("<td></td>")
    s, o = lines[r]
    rows.append(f"<tr><th>{esc(r)}</th><td class='n'>{s:,}</td>{''.join(cells)}</tr>")
gui_note = "smelt-gui (excluded from the workspace, own Cargo workspace) depends on smelt-frontend-ts, smelt-frontend-py (optional, feature python), smelt-hir, smelt-mir and smelt-codegen-rust by version, patched to this repository."
share = lambda c: 100 * member_lines[c][0] / total_src
P(f"""<section id="crates"><h2>2. Crate dependency graph</h2>
{fig(s2, "hoverable")}
<p>Edges are the normal dependencies in each <code>Cargo.toml</code> (via <code>cargo metadata</code>); a pale edge is a direct dependency that is also reachable through another crate. Hover a crate to highlight its edges. The bar is lines of Rust under <code>src/</code>; {total_src:,} lines in the twelve members, of which smelt-frontend-ts holds {share("smelt-frontend-ts"):.0f}% and smelt-codegen-rust {share("smelt-codegen-rust"):.0f}%, against {share("smelt-mir"):.0f}% for smelt-mir.
Features cut the graph at three dashed edges: smelt-transpiler's defaults are <code>ty</code> and <code>ts7</code>, so <code>--no-default-features</code> drops smelt-frontend-py, smelt-py-types and smelt-ts-types (tsrs) from the CLI, and TypeScript then lowers from annotations alone ({a("crates/smelt-transpiler/src/ts_facts.rs", "ts_facts.rs")}). smelt-codegen-rust takes smelt-runtime with <code>default-features = false</code>: the text, not the compiled runtime. <code>default-members</code> leaves out smelt-ts-types, smelt-ts-model and smelt-py-types.</p>
<details><summary>Dependency matrix (rows depend on columns), dev-dependencies and smelt-gui</summary>
<div class="tablewrap"><table class="matrix"><thead><tr><th></th><th>src lines</th>{head}</tr></thead><tbody>{"".join(rows)}</tbody></table></div>
<p>Dev-only edges (not drawn): {", ".join(f"{esc(x)} → {esc(y)}" for x, y in dev)}. {esc(gui_note)}</p>
</details>
</section>""")

# 3
def toptable(lst, title):
    r = "".join(f"<tr><td class='n'>{i}</td><td class='n'>{n:,}</td><td>{a(f)}</td></tr>" for i, (n, f) in enumerate(lst, 1))
    return f"<div class='tablewrap'><table class='plain'><caption>{esc(title)}</caption><thead><tr><th>#</th><th>lines</th><th>file</th></tr></thead><tbody>{r}</tbody></table></div>"


P(f"""<section id="modules"><h2>3. Inside the big crates</h2>
<p class="lead">One row per responsibility; bar length is lines of Rust on one scale shared by every crate (760 px = 40k lines), so rows compare across crates. Segments are modules (a file plus its directory); hover for exact counts. Numbered circles mark the ten largest non-test files. Each row names its entry function and links to its file.</p>""")
names = {"smelt-frontend-ts": "smelt-frontend-ts", "smelt-codegen-rust": "smelt-codegen-rust", "smelt-frontend-py": "smelt-frontend-py",
         "smelt-mir": "smelt-mir", "smelt-runtime": "smelt-runtime", "smelt-transpiler": "smelt-transpiler"}
for c, (svgt, total, mrows, groups) in maps.items():
    body = "".join(f"<tr><td>{esc(gname)}</td><td><code>{esc(k)}</code></td><td class='n'>{n:,}</td><td class='n'>{f}</td></tr>" for gname, k, n, f in mrows)
    P(f"""<h3 id="m-{c}">{esc(c)} <span class="sub">{total:,} lines under src/</span></h3>
{fig(svgt)}
<details><summary>Every module of {esc(c)} with its line and file count</summary><div class="tablewrap"><table class="plain"><thead><tr><th>group</th><th>module</th><th>lines</th><th>files</th></tr></thead><tbody>{body}</tbody></table></div></details>""")
P(f"""<div class="two">{toptable(prodf, "Ten largest non-test files (the numbered circles)")}{toptable(allf, "Ten largest files of all {:,} tracked .rs files".format(nfiles))}</div>
<p>smelt-hir is not mapped here (11.4k lines); its largest file, {a("crates/smelt-hir/src/op_table.rs")}, is the X-macro table every operation shared by HIR and MIR is declared in.</p>
</section>""")

# 4
es, re_ = summ["es"], summ["re"]
P(f"""<section id="types"><h2>4. The life of one value's type</h2>
{fig(s4)}
<p>A type is decided once, in the frontend, and carried by <code>TypeId</code> through HIR and MIR (MIR clones HIR's interner) to <code>FunctionEmitter::rust_type</code>. The trace above is a golden example: <code>Config['tags']</code> is a type-level form the frontend does not evaluate, so it erases with <code>type-level</code>; tsrs answers the span with <code>string[]</code>, the record is dropped and the value stays <code>SmeltList&lt;JsString&gt;</code>. The bars are erasure records per reason from the committed baselines (es-toolkit {gitdate("blocker-logs/smelt-unknown-baseline-es-toolkit.json")}, remeda {gitdate("blocker-logs/smelt-unknown-baseline-remeda.json")}): es-toolkit {summ["es_records"]:,} records, {summ["es_avoidable"]:,} avoidable, {summ["es_records"] - summ["es_backend"]:,} decided by the frontend and {summ["es_backend"]:,} by codegen; remeda {summ["re_records"]:,}, {summ["re_avoidable"]:,} avoidable, {summ["re_records"] - summ["re_backend"]:,} frontend and {summ["re_backend"]:,} codegen. The largest avoidable causes are <code>explicit-any</code> in es-toolkit ({es["explicit-any"][0]:,}: the source opts out), then <code>erased-generic</code> ({es["erased-generic"][0]:,} / {re_["erased-generic"][0]:,}: a type parameter no Rust generic carries), <code>type-level</code>, <code>missing-annotation</code> and <code>erased-union</code>. "Code mentions" counts <code>ErasureReason::X</code> in non-test frontend code, a proxy for decision sites. MIR's two retypings to <code>Unknown</code> record no reason, so the sidecar cannot attribute them.</p>
</section>""")

# 5
P(f"""<section id="mir"><h2>5. MIR in one picture</h2>
{fig(s5a)}
{fig(s5b)}
<p>MIR is a CFG over mutable, typed locals (not SSA). Only <code>Call</code> and <code>Await</code> carry an unwind edge, so anything that can throw is one of them; a function whose throw escapes gets <code>can_throw</code> and codegen returns a <code>Result</code>. Order read from {a("crates/smelt-mir/src/compile.rs", "compile.rs")}, {a("crates/smelt-mir/src/lower/mod.rs", "lower/mod.rs")} (<code>lower_hir</code>, <code>run_finalization_passes</code>) and {a("crates/smelt-mir/src/opt/mod.rs", "opt/mod.rs")} (<code>fixpoint_passes</code>, <code>try_optimize</code>).</p>
</section>""")

# 6
def bl(path):
    return json.load(open(f"{ROOT}/{path}"))


ub = {k: bl(f"blocker-logs/smelt-unknown-baseline{k}.json") for k in ("", "-es-toolkit", "-remeda", "-hono")}
tb = {k: bl(f"blocker-logs/typedness-baseline{k}.json") for k in ("", "-es-toolkit", "-remeda")}
esci, honoci = bl("blocker-logs/es-toolkit-ci-baseline.json"), bl("blocker-logs/hono-ci-baseline.json")
clippy = bl("blocker-logs/clippy-baseline.json")
clippy_total = sum(sum(v.values()) for v in clippy["counts"].values())


def ucell(k):
    d = ub[k]
    ct = d["category_totals"]
    er = d.get("erasure")
    return (f"<td class='n'>{d['files_scanned']:,}</td><td class='n'>{ct.get('avoidable-erasure', 0):,}</td>"
            f"<td class='n'>{ct.get('legitimate-boundary', 0):,}</td><td class='n'>{er['records']:,} / {er['avoidable']:,}</td>" if er else
            f"<td class='n'>{d['files_scanned']:,}</td><td class='n'>{ct.get('avoidable-erasure', 0):,}</td><td class='n'>{ct.get('legitimate-boundary', 0):,}</td><td class='n'>–</td>")


def tcell(k):
    t = tb[k]["total"]
    f = lambda kind: f"{t[kind]['erased']:,} of {sum(t[kind].values()):,}"
    return f"<td class='n'>{f('values')}</td><td class='n'>{f('reads')}</td><td class='n'>{f('calls')}</td>"


P(f"""<section id="gates"><h2>6. How quality is gated</h2>
{fig(s6)}
<p>Read from {a(".github/workflows/ci.yml", "ci.yml")}. An ordinary PR push runs only the left column, on the packages its diff touches; the library corpora, the runtime tiers and every SmeltUnknown and typedness gate run in FULL mode, so a codegen PR without the <code>run-regressions</code> label meets them first on <code>main</code>.</p>
<div class="tablewrap"><table class="plain"><caption>SmeltUnknown baselines: generated-line categories and erasure records (sidecar)</caption>
<thead><tr><th>baseline</th><th>gate</th><th>committed</th><th>files</th><th>avoidable lines</th><th>legitimate lines</th><th>records / avoidable</th></tr></thead><tbody>
<tr><td>{a("blocker-logs/smelt-unknown-baseline.json", "examples")}</td><td>blocks (avoidable = 0)</td><td>{gitdate("blocker-logs/smelt-unknown-baseline.json")}</td>{ucell("")}</tr>
<tr><td>{a("blocker-logs/smelt-unknown-baseline-es-toolkit.json", "es-toolkit")}</td><td>blocks (ratchet)</td><td>{gitdate("blocker-logs/smelt-unknown-baseline-es-toolkit.json")}</td>{ucell("-es-toolkit")}</tr>
<tr><td>{a("blocker-logs/smelt-unknown-baseline-remeda.json", "remeda")}</td><td>advisory</td><td>{gitdate("blocker-logs/smelt-unknown-baseline-remeda.json")}</td>{ucell("-remeda")}</tr>
<tr><td>{a("blocker-logs/smelt-unknown-baseline-hono.json", "hono")}</td><td>advisory</td><td>{gitdate("blocker-logs/smelt-unknown-baseline-hono.json")}</td>{ucell("-hono")}</tr>
</tbody></table></div>
<div class="tablewrap"><table class="plain"><caption>Typedness baselines: erased of all counted, per kind</caption>
<thead><tr><th>baseline</th><th>gate</th><th>committed</th><th>values</th><th>property reads</th><th>calls</th></tr></thead><tbody>
<tr><td>{a("blocker-logs/typedness-baseline.json", "examples")}</td><td>blocks</td><td>{gitdate("blocker-logs/typedness-baseline.json")}</td>{tcell("")}</tr>
<tr><td>{a("blocker-logs/typedness-baseline-es-toolkit.json", "es-toolkit")}</td><td>blocks</td><td>{gitdate("blocker-logs/typedness-baseline-es-toolkit.json")}</td>{tcell("-es-toolkit")}</tr>
<tr><td>{a("blocker-logs/typedness-baseline-remeda.json", "remeda")}</td><td>advisory</td><td>{gitdate("blocker-logs/typedness-baseline-remeda.json")}</td>{tcell("-remeda")}</tr>
</tbody></table></div>
<div class="tablewrap"><table class="plain"><caption>Other committed marks</caption>
<thead><tr><th>file</th><th>committed</th><th>marks</th></tr></thead><tbody>
<tr><td>{a("blocker-logs/es-toolkit-ci-baseline.json", "es-toolkit-ci-baseline.json")}</td><td>{gitdate("blocker-logs/es-toolkit-ci-baseline.json")}</td><td>probe blockers {esci["probe_blockers"]}, check errors {esci["check_errors"]}, test build errors {esci["test_build_errors"]}, runtime passed ≥ {esci["runtime_passed"]:,}, {len(esci["runtime_known_failures"])} pinned failures; warnings {esci["warning_count"]} (advisory)</td></tr>
<tr><td>{a("blocker-logs/hono-ci-baseline.json", "hono-ci-baseline.json")}</td><td>{gitdate("blocker-logs/hono-ci-baseline.json")}</td><td>test compile errors {honoci["test_compile_errors"]}, runtime passed ≥ {honoci["runtime_passed"]}, {len(honoci["runtime_known_failures"])} pinned failures</td></tr>
<tr><td>{a("blocker-logs/clippy-baseline.json", "clippy-baseline.json")}</td><td>{gitdate("blocker-logs/clippy-baseline.json")}</td><td>{clippy_total:,} warn-level lint hits across {len(clippy["counts"])} crates; may only fall</td></tr>
<tr><td>{a(".github/workflows/ci.yml", "ci.yml")} pins</td><td>{gitdate(".github/workflows/ci.yml")}</td><td>remeda 1789 passed / 0 failed; radash 381 / 6; ts-pattern 382 / 56; date-fns 273 / 57</td></tr>
</tbody></table></div>
</section>""")

# 7
FT = "crates/smelt-frontend-ts/src/"
CG = "crates/smelt-codegen-rust/src/"
MR = "crates/smelt-mir/src/"
c = lambda *m: commits(*m)
LOOK = [
    ("Where are stdlib and host calls mapped (TypeScript)?", [(FT + "lowering/stdlib/call_dispatch.rs", "ModuleBuilder::call_expression_with_hint"), ("crates/smelt-stdlib/src/recognition.rs", "typescript_call_rule")], c(FT + "lowering/stdlib")),
    ("Where does a value cross between a typed Rust value and SmeltUnknown?", [(CG + "emitter/coercion.rs", "value_at_type / rendered_at_type"), (CG + "emitter/coercion/erase.rs", "erase"), (CG + "emitter/coercion/extract.rs", "extract")], c(CG + "emitter/coercion")),
    ("Where are expressions lowered?", [(FT + "lowering/expr/dispatch.rs", "ModuleBuilder::expression_with_hint")], c(FT + "lowering/expr")),
    ("Where are classes and functions declared?", [(FT + "lowering/decls/classes.rs", "class_declaration"), (FT + "lowering/decls/functions.rs", "function_declaration")], c(FT + "lowering/decls")),
    ("Where does a TS annotation become a HIR type?", [(FT + "lowering/ty/annotations.rs", "ModuleBuilder::ts_type_to_hir")], c(FT + "lowering/ty")),
    ("Where do tsrs facts overturn an erased annotation?", [(FT + "lowering/ty/annotations.rs", "checker_evaluated_or"), (FT + "lowering/checker_types.rs", "checker_annotation_type")], c(FT + "lowering/checker_types")),
    ("Where are module top-level statements lowered?", [(FT + "lowering/module_init.rs", "ModuleBuilder::program")], c(FT + "lowering/module_init")),
    ("Where is an arrow passed as an argument lowered?", [(FT + "lowering/callbacks/dispatch.rs", "callback_argument")], c(FT + "lowering/callbacks")),
    ("Where are statements lowered?", [(FT + "lowering/stmt/statements.rs", "statement_in_block")], c(FT + "lowering/stmt")),
    ("Where is new X(..) lowered?", [(FT + "lowering/new_expr.rs", "new_expression_with_hint")], c(FT + "lowering/new_expr")),
    ("Where is a MIR block emitted as Rust?", [(CG + "emitter/control_flow.rs", "FunctionEmitter::emit_block")], c(CG + "emitter/control_flow")),
    ("Where is async lowered?", [(FT + "lowering/expr/dispatch.rs", "expression_with_hint (AwaitExpression)"), (MR + "lower/expr.rs", "lower_expr → Terminator::Await"), (CG + "emitter/control_flow/calls.rs", "emit_await_terminator_statement"), (CG + "emitter/async_op.rs", "async_op_expr (Promise, timers)")], c(CG + "emitter/async_op")),
    ("Where is the runtime text chosen?", [(CG + "runtime_demand.rs", "RuntimeDemand::of"), (CG + "runtime_prelude.rs", "emit_runtime_prelude"), ("crates/smelt-runtime/src/order.rs", "ORDER")], c(CG + "runtime_demand", CG + "runtime_prelude")),
    ("Where are union types emitted?", [(CG + "emitter/union.rs", "emit_union_definitions, union_rust_type")], c(CG + "emitter/union")),
    ("Where does a test file become #[test]?", [(FT + "lowering/testing/suites.rs", "test_function_from_arrow (is_test)"), (CG + "emitter/function_item.rs", "emit_free_function"), ("crates/smelt-frontend-py/src/lowering/pytest.rs", "pytest")], c(FT + "lowering/testing")),
    ("Where are typeof / instanceof narrowings?", [(FT + "lowering/guards.rs", "instanceof_test"), (FT + "lowering/narrowing.rs", "guard_narrowing")], c(FT + "lowering/guards", FT + "lowering/narrowing")),
    ("Where does a closure get its captures and ABI?", [(MR + "lower/closures.rs", "lower_closure_expr, mark_escaping_closures"), (CG + "emitter/closures.rs", "closure_expr")], c(CG + "emitter/closures")),
    ("Where is a HIR type rendered as Rust, and backend erasure decided?", [(CG + "emitter/types.rs", "FunctionEmitter::rust_type"), (CG + "emitter/backend_erasure.rs", "note_backend_erasure")], c(CG + "emitter/types")),
    ("Where does a frontend record an erasure?", [(FT + "lowering/erasure.rs", "ModuleBuilder::erase, erase_at"), ("crates/smelt-frontend-py/src/lowering/type_helpers.rs", "erase")], c(FT + "lowering/erasure")),
    ("Where do I add an operation shared by HIR and MIR?", [("crates/smelt-hir/src/op_table.rs", "for_each_op!")], c("crates/smelt-hir/src/op_table")),
    ("Where is a MIR optimization registered?", [(MR + "opt/mod.rs", "fixpoint_passes, try_optimize")], c(MR + "opt/mod")),
    ("Where are free-function generics decided?", [(CG + "generic_lifting.rs", "function_emits_rust_generics")], c(CG + "generic_lifting")),
    ("Where does a throw become a Result?", [(MR + "lower/passes/throwing.rs", "propagate_throwing_functions"), (CG + "thrown.rs", "throw_expr")], c(MR + "lower/passes", CG + "thrown")),
    ("Where are the SmeltUnknown and typedness numbers computed?", [("crates/smelt-transpiler/src/unknown_report/classify.rs", "classify_line"), (CG + "lib.rs", "measure_typedness"), ("crates/smelt-transpiler/src/typedness_report.rs", "TypednessReport::from_programs")], c("crates/smelt-transpiler/src/unknown_report")),
    ("Where are golden examples compared?", [("crates/smelt-transpiler/tests/hir_cli_cross_language_tests.rs", "end_to_end_examples_match_expected_outputs")], c("crates/smelt-transpiler/tests")),
    ("Where does the CLI start a build?", [("crates/smelt-transpiler/src/cli.rs", "run"), ("crates/smelt-transpiler/src/pipeline.rs", "build_rust_crate_with_erasure")], c("crates/smelt-transpiler/src/cli", "crates/smelt-transpiler/src/pipeline")),
    ("Where is a Python class lowered?", [("crates/smelt-frontend-py/src/lowering/class.rs", "lower_class_def")], c("crates/smelt-frontend-py/src/lowering/class")),
]
trs = "".join(
    f"<tr><td>{esc(q)}</td><td>{'<br>'.join(a(p, (p.split('/')[1].replace('smelt-', '') + ': ' + p.split('/src/', 1)[1]) if '/src/' in p else p) + ' <span class=fn>' + esc(fn) + '</span>' for p, fn in ans)}</td><td class='n'>{n}</td></tr>"
    for q, ans, n in LOOK)
P(f"""<section id="lookup"><h2>7. Where does X live?</h2>
<p class="lead">Questions picked from the areas with the most commits since {subprocess.check_output(["git", "log", "--since=60 days ago", "--no-merges", "--reverse", "--format=%cs"], cwd=ROOT).decode().split()[0]} (last column: non-merge commits touching the module, 60 days). Paths read <code>crate: path under src/</code>.</p>
<div class="tablewrap"><table class="plain lookup"><thead><tr><th>question</th><th>file and function</th><th>commits</th></tr></thead><tbody>{trs}</tbody></table></div>
</section>""")

# 8
top = sorted(pts, key=lambda r: -r["commits"])[:25]
toprows = "".join(f"<tr><td><code>{esc(s8_heat.short(r['module']))}</code></td><td class='n'>{r['lines']:,}</td><td class='n'>{r['commits']}</td><td class='n'>{r['changed']:,}</td></tr>" for r in top)
ch = {m: churn[m] for m in s8_heat.HOT}
st = ch["crates/smelt-frontend-ts/src/lowering/stdlib"]
co = ch["crates/smelt-codegen-rust/src/emitter/coercion"]
ex = ch["crates/smelt-frontend-ts/src/lowering/expr"]
nmerge = subprocess.check_output(["git", "rev-list", "--count", "--no-merges", "--since=60 days ago", "HEAD"], cwd=ROOT).decode().strip()
P(f"""<section id="heat"><h2>8. Heat map</h2>
{fig(s8)}
<p>{nmerge} non-merge commits since {subprocess.check_output(["git", "log", "--since=60 days ago", "--no-merges", "--reverse", "--format=%cs"], cwd=ROOT).decode().split()[0]}. A module is a file plus its directory, one level deeper inside <code>lowering/</code>, <code>emitter/</code>, <code>runtime/</code>, <code>lower/</code> and <code>opt/</code>; test directories are left out. Ringed: the three to look at first.</p>
<ol class="hot">
<li><b>{a("crates/smelt-frontend-ts/src/lowering/stdlib", "frontend-ts lowering/stdlib")}</b>: {st["lines"]:,} lines, {st["commits"]} commits, {st["changed"]:,} lines changed. The largest and hottest module; {a("crates/smelt-frontend-ts/src/lowering/stdlib/call_dispatch.rs", "call_dispatch.rs")} alone took 66 commits. Every host API is recognised here, and it holds the most <code>untyped-builtin</code>, <code>external-type</code> and <code>unmodeled-type</code> erase sites, so it is where per-API special cases would accumulate and where typed host models pay off first.</li>
<li><b>{a("crates/smelt-codegen-rust/src/emitter/coercion.rs", "codegen emitter/coercion")}</b>: {co["lines"]:,} lines, {co["commits"]} commits. The one typed ↔ <code>SmeltUnknown</code> seam; {a("crates/smelt-codegen-rust/src/emitter/coercion/function_adapter.rs", "function_adapter.rs")} is the 7th largest file. Churn here follows erasure upstream: each new erased shape needs an adapter, so it should cool as section 4's avoidable counts fall. A rise is an early signal of erasure leaking.</li>
<li><b>{a("crates/smelt-frontend-ts/src/lowering/expr", "frontend-ts lowering/expr")}</b>: {ex["lines"]:,} lines, {ex["commits"]} commits; {a("crates/smelt-frontend-ts/src/lowering/expr/references.rs", "references.rs")} (3rd largest file) and {a("crates/smelt-frontend-ts/src/lowering/expr/member_access.rs", "member_access.rs")}. It holds the most <code>by-name</code> and <code>dynamic-operator</code> sites, the two frontend reasons with the most code mentions after <code>missing-annotation</code>.</li>
</ol>
<p>Moves inside the window: <code>emitter/core.rs</code> and <code>emitter/call_runtime.rs</code> were split by concern on 2026-11-07, and the <code>*_prelude.rs</code> runtime strings left smelt-codegen-rust for smelt-runtime; their history shows as churn on files that are now small or gone (gone ones are not plotted).</p>
<details><summary>Top 25 modules by commits</summary><div class="tablewrap"><table class="plain"><thead><tr><th>module</th><th>lines</th><th>commits</th><th>lines changed</th></tr></thead><tbody>{toprows}</tbody></table></div></details>
</section>""")

P(f"""<section id="notes"><h2>Notes</h2>
<ul>
<li>Line counts are <code>wc -l</code> over git-tracked <code>.rs</code> files and include inline tests and comments.</li>
<li>Left out because it could not be verified without running the compiler: per-pass timings, how often each MIR pass fires, op counts of the op table, and erasure counts for programs outside the committed baselines.</li>
<li>Text docs that disagree with the code at this commit:
<ul>
<li>{a("specs/architecture.md")} calls smelt-runtime "a small Rust crate that transpiled code depends on at runtime"; generated crates inline its text and never depend on it ({a("crates/smelt-runtime/src/lib.rs", "lib.rs")}), and it is 19.8k lines.</li>
<li>{a("specs/architecture.md")} says input must be strict with no <code>any</code>; <code>any</code> lowers to <code>SmeltUnknown</code> with reason <code>explicit-any</code> (1,180 es-toolkit records) and <code>--allow-type-errors</code> opts out of the type gate.</li>
<li>{a("specs/architecture.md")} has smelt-frontend-ts run "oxclint → tsgo --noEmit"; nothing in <code>crates/</code> invokes tsgo or oxlint. The checker is tsrs, embedded by smelt-ts-types and run by the transpiler's type gate. Its crate list also omits smelt-stdlib, smelt-specialize, smelt-ts-types, smelt-ts-model and smelt-py-types, and "ownership inference is a v2.0 problem" predates <code>MoveOnLastUse</code>.</li>
<li>{a("specs/mir.md")} lists six finalization steps; the code runs nine (it omits <code>check_unbound_bindings</code>, <code>remove_unreachable_blocks</code> and <code>abstract_classes_as_unions</code>), its validator list omits <code>tables</code> and the type check, and validation fails <code>compile</code> in release builds too, not only during development.</li>
<li>{a("crates/smelt-transpiler/Cargo.toml")} describes feature <code>ts7</code> as being "for smelt ts7-parity-report"; it also decides whether the type gate has checker facts at all ({a("crates/smelt-transpiler/src/ts_facts.rs", "ts_facts.rs")}).</li>
</ul></li>
</ul>
</section>
</main>
<script>{JS}</script>
</body>
</html>
""")

html_text = "".join(parts)
os.makedirs(os.path.dirname(OUT), exist_ok=True)
open(OUT, "w").write(html_text)
missing = [p for p in sorted(svgmod.LINKS) if not os.path.exists(os.path.join(ROOT, p))]
print("written", OUT, len(html_text), "bytes; links", len(svgmod.LINKS), "missing", missing)
