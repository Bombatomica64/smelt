"""Section 6: how quality is gated (from .github/workflows/*.yml)."""
from svg import Svg, L, H, S

CI = ".github/workflows/ci.yml"


def build():
    g = Svg(1040, 1000, "CI jobs on pull requests and on main")
    LX, LW = 16, 400
    RX, RW = 440, 584
    t1 = g.node(LX, 16, LW, 78, [H("QUICK  ·  every pull_request push"), S("build only the packages a diff touches"),
                                 S("(scripts/ci_affected_packages.py)")], kind="front", href=CI)
    t2 = g.node(RX, 16, RW, 78, [H("FULL  ·  push to main, nightly 02:23 UTC, dispatch,"),
                                 S("or a PR labelled run-regressions (regressions-label.yml"),
                                 S("dispatches it on the PR head at once)")], kind="codegen", href=".github/workflows/regressions-label.yml")

    def job(x, y, w, title, steps, h=None):
        lines = [H(title)]
        for kind, text in steps:
            lines.append(L(("■ " if kind == "B" else "□ " if kind == "A" else "· ") + text,
                           cls={"B": "block", "A": "adv", "": ""}[kind]))
        hh = h or (20 + 18 + 17 * (len(lines) - 1) + 4)
        return g.node(x, y, w, hh, lines, kind="gate", href=CI, cls="node job", gap=4)

    y = 128
    g.add(g.text(LX + 44, y - 8, S("runs in both modes")))
    a = job(LX, y, LW, "ci-gates", [("B", "scripts unittest, check_ci_gates.py"), ("B", "cargo fmt --check")])
    b = job(LX, a["b"] + 14, LW, "build (the only job that compiles)", [
        ("B", "cargo nextest archive"), ("B", "clippy: deny lints + ratchet"), ("", "  blocker-logs/clippy-baseline.json"),
        ("B", "doctests")])
    c = job(LX, b["b"] + 14, LW, "test  ×4 shards (from the archive)", [
        ("B", "unit + integration tests"), ("B", "golden examples: 168 TS dirs,"),
        ("", "  expected.{hir,mir,rs,stdout}"), ("", "  end_to_end_examples_match_"),
        ("", "  expected_outputs")])
    for p, q in ((t1, a), (a, b), (b, c)):
        g.path(f"M{LX + 30},{p['b']} V{q['y'] - 2}")
    g.path(f"M{RX + 30},{t2['b']} V{y - 20} H{LX + LW - 30} V{a['y'] - 2}")

    g.add(g.text(RX, y - 8, S("FULL only — each job reuses build's archive or smelt binary")))
    cw = (RW - 12) / 2
    r1 = job(RX, y, cw, "corpus", [("B", "compile_corpus + shape grid"), ("B", "SmeltUnknown examples:"),
                                   ("", "  avoidable == 0"), ("B", "typedness examples ratchet"), ("B", "smelt-schema.json")])
    r2 = job(RX + cw + 12, y, cw, "runtime-tiers ×4", [("B", "every *_runtime test binary"), ("", "  runs generated crates and"),
                                                     ("", "  checks expect(..) at runtime")], h=r1["h"])
    es = job(RX, r1["b"] + 14, RW, "es-toolkit-regression (es-toolkit-ci-baseline.json)", [
        ("B", "probe blockers ≤ 0, cargo check errors = 0, test build errors = 0"),
        ("B", "runtime ratchet: ≥ 1048 passed, only 11 pinned failures"),
        ("B", "SmeltUnknown ratchet  (smelt-unknown-baseline-es-toolkit.json)"),
        ("B", "typedness ratchet  (typedness-baseline-es-toolkit.json)")])
    rm = job(RX, es["b"] + 14, cw, "remeda-regression", [("B", "1789 passed; 0 failed"), ("A", "SmeltUnknown report"),
                                                       ("A", "typedness report")])
    hn = job(RX + cw + 12, es["b"] + 14, cw, "hono", [("B", "crate cargo check = 0"), ("B", "test build errors = 0,"),
                                                    ("", "  ≥ 444 passed, 10 pinned"), ("A", "SmeltUnknown report")])
    pin = job(RX, max(rm["b"], hn["b"]) + 14, RW, "pinned test-result lines (grep -F)", [
        ("B", "radash 381 passed; 6 failed    ts-pattern 382 passed; 56 failed"),
        ("B", "date-fns 273 passed; 57 failed")])
    misc = job(RX, pin["b"] + 14, RW, "sandbox, package", [("B", "specialization tests with SMELT_REQUIRE_SANDBOX=1"),
                                                          ("B", "scripts/publish-crates.sh (package every crate)")])
    for p in (r1, r2):
        pass
    # legend + other workflows
    ly = max(c["b"], 0) + 30
    lg = g.node(LX, ly, LW, 96, [H("Legend"), L("■ blocking step", cls="block"), L("□ advisory (|| true, never fails)", cls="adv"),
                                 S("Docs-only diffs (*.md, docs/**, *.svg) skip CI.")], kind="note", cls="node note")
    ow = g.node(LX, lg["b"] + 14, LW, 156, [H("Other workflows"), L("coverage.yml   main push, nightly 03:41"),
                                            L("pages.yml      main push"), L("shape-grid.yml nightly 03:40, full grid"),
                                            L("library-probes nightly 06:17, commits"), L("compat, publish-*  manual")],
                kind="note", href=".github/workflows/coverage.yml", cls="node note")
    g.h = max(ow["b"], misc["b"]) + 16
    return g.render()
