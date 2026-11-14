"""Section 4: the life of one value's type, and where it can be lost."""
import json
from svg import Svg, L, H, S, esc, tw
from sites import sites

ROOT = __import__("subprocess").check_output(["git", "rev-parse", "--show-toplevel"], cwd=__import__("os").path.dirname(__import__("os").path.abspath(__file__))).decode().strip()
BACKEND = {"erased-generic", "erased-union", "unresolved-class", "never-value"}


def kebab_to_variant(k):
    return "".join(p.capitalize() for p in k.split("-"))


def load(corpus):
    d = json.load(open(f"{ROOT}/blocker-logs/smelt-unknown-baseline-{corpus}.json"))
    e = d["erasure"]
    return {r["reason"]: (r["count"], r["class"]) for r in e["by_reason"]}, e


def build():
    es, ese = load("es-toolkit")
    re_, ree = load("remeda")
    import re as _re
    src = open(f"{ROOT}/crates/smelt-hir/src/erasure.rs").read()
    table = _re.findall(r'info\(\s*ErasureReason::(\w+),\s*"([\w-]+)",\s*"[^"]*",\s*ErasureClass::(\w+)', src)
    reasons = [(name, cls.lower()) for _, name, cls in table]
    st = sites()
    W = 1040
    g = Svg(W, 1000, "How a TypeScript annotation becomes a Rust type, and where it can be erased")

    # stage lane: a real trace from examples/typescript/end-to-end/172_checker_evaluated_annotation_types
    xs = [16, 219, 422, 625, 828]
    BW, BH, Y = 188, 118, 40
    g.add(g.text(16, 24, S("Trace: examples/typescript/end-to-end/172_checker_evaluated_annotation_types (input.ts, expected.hir/.mir/.rs)", 13)))
    ex = "examples/typescript/end-to-end/172_checker_evaluated_annotation_types/"
    st1 = g.node(xs[0], Y, BW, BH, [H("Source"), L("const tags:"), L("  Config['tags']"), L("  = ['a','b','c']")],
                 kind="driver", href=ex + "input.ts")
    st2 = g.node(xs[1], Y, BW, BH, [H("Frontend"), L("ts_type_to_hir"), L("indexed access on"), L("an interface →"),
                                    L("erase(TypeLevel)", cls="loss")],
                 kind="front", href="crates/smelt-frontend-ts/src/lowering/ty/annotations.rs")
    st3 = g.node(xs[2], Y, BW, BH, [H("HIR"), L("%2 const tags:"), L("  List<String>"), S("TypeInterner id")],
                 kind="hir", href=ex + "expected.hir")
    st4 = g.node(xs[3], Y, BW, BH, [H("MIR"), L("%2 user tags:"), L("  List<String>"), S("LocalDecl.ty")],
                 kind="mir", href=ex + "expected.mir")
    st5 = g.node(xs[4], Y, BW, BH, [H("Rust"), L("rust_type(id) →"), L("let tags:"), L("SmeltList<JsString>")],
                 kind="codegen", href=ex + "expected.rs")
    for a, b in ((st1, st2), (st2, st3), (st3, st4), (st4, st5)):
        g.path(f"M{a['r']},{a['cy']:.0f} H{b['x'] - 2}")
    # rescue
    rs = g.node(xs[1] + 60, Y + BH + 46, 450, 78, [
        L("checker_evaluated_or → checker_annotation_type", cls="good"),
        S("tsrs evaluates the span to string[]; the TypeLevel record"),
        S("is dropped. Without facts (no ts7, SMELT_TS7=0) it stays."),
    ], kind="good", href="crates/smelt-frontend-ts/src/lowering/checker_types.rs", cls="node good")
    g.path(f"M{st2['cx'] + 40:.0f},{rs['y']} V{st2['b'] + 2}", cls="edge good", marker="arr-good")
    g.path(f"M{st3['cx']:.0f},{rs['y']} V{st3['b'] + 2}", cls="edge good", marker="arr-good")

    # loss blocks
    top = 300
    lx = 16

    fy = top
    FX, FW = 16, 600
    rows_f = [r for r in reasons if r[0] not in BACKEND]
    rows_b = [r for r in reasons if r[0] in BACKEND]
    key = lambda r: (r[1] != "avoidable", -(es.get(r[0], (0,))[0] + re_.get(r[0], (0,))[0]))
    rows_f.sort(key=key)
    rows_b.sort(key=key)
    RH = 26
    hh = 46
    g.node(FX, fy, FW, hh + RH * len(rows_f) + 10, [H("Frontends  ·  erase(reason) / erase_at(..)")], kind="front",
           href="crates/smelt-frontend-ts/src/lowering/erasure.rs", cls="node panel")
    maxc = max(max(v[0] for v in es.values()), max(v[0] for v in re_.values()))
    BARW = 70

    def header(x0, y0):
        g.add(g.text(x0 + 8, y0, S("reason", 13)))
        g.add(g.text(x0 + 168, y0, S("code mentions · top module", 13)))
        g.add(g.text(x0 + 368, y0, S("es-toolkit", 13)))
        g.add(g.text(x0 + 484, y0, S("remeda", 13)))

    def row(x0, y0, name, cls):
        variant = kebab_to_variant(name)
        mentions = st.get(variant, {})
        n = sum(mentions.values())
        topm = mentions.most_common(1)[0][0][1].split("/")[-1] if mentions else "-"
        g.add(g.text(x0 + 8, y0 + 17, L(name, cls="loss" if cls == "avoidable" else "")))
        g.add(g.text(x0 + 168, y0 + 17, L(f"{n:>3} · {topm}", cls="t2")))
        for off, data in ((368, es), (484, re_)):
            c = data.get(name, (0, cls))[0]
            w = BARW * c / maxc
            g.add(f'<rect class="lg {"loss" if cls == "avoidable" else "legit"}" x="{x0 + off}" y="{y0 + 6}" width="{max(w, 1):.1f}" height="14" rx="2">'
                  f'<title>{name}: {c} records</title></rect>')
            g.add(g.text(x0 + off + max(w, 1) + 4, y0 + 18, L(str(c))))
            assert off + w + 4 + tw(str(c), 13) < (484 if off == 368 else FW - 4), (name, c)

    header(FX, fy + hh - 6)
    for i, (name, cls) in enumerate(rows_f):
        row(FX, fy + hh + 2 + i * RH, name, cls)

    # MIR block
    MX, MW = 632, 392
    my = fy
    g.node(MX, my, MW, 132, [
        H("MIR  ·  no ErasureRecord, two retypings"),
        L("promote_erased_mutated_records"),
        S("Dict(K, V) → Dict(K, unknown) when a record"),
        S("is both erased and mutated (reference identity)"),
        L("lower_hir: pending thrown payload"),
        S("of try/finally is typed Unknown"),
    ], kind="mir", href="crates/smelt-mir/src/erased_record_promote.rs", cls="node panel")
    # backend
    by = my + 148
    bh = hh + RH * len(rows_b) + 10
    g.node(MX, by, MW, bh, [H("Codegen  ·  FunctionEmitter::rust_type")], kind="codegen",
           href="crates/smelt-codegen-rust/src/emitter/backend_erasure.rs", cls="node panel")
    bx0 = MX - 160  # reuse row geometry, shifted so columns land inside the panel

    def brow(y0, name, cls):
        c1 = es.get(name, (0,))[0]
        c2 = re_.get(name, (0,))[0]
        g.add(g.text(MX + 8, y0 + 17, L(name, cls="loss")))
        for off, c in ((160, c1), (278, c2)):
            w = 70 * c / maxc
            g.add(f'<rect class="lg loss" x="{MX + off}" y="{y0 + 6}" width="{max(w, 1):.1f}" height="14" rx="2"><title>{name}: {c} records</title></rect>')
            g.add(g.text(MX + off + max(w, 1) + 4, y0 + 18, L(str(c))))
            assert off + w + 4 + tw(str(c), 13) < (278 if off == 160 else MW - 4), (name, c)
    g.add(g.text(MX + 160, by + hh - 6, S("es-toolkit")))
    g.add(g.text(MX + 278, by + hh - 6, S("remeda")))
    for i, (name, cls) in enumerate(rows_b):
        brow(by + hh + 2 + i * RH, name, cls)
    cy = by + bh + 16
    g.node(MX, cy, MW, 96, [
        H("Codegen  ·  coercion seam"),
        L("value_at_type / erase / extract"),
        S("renders each typed ↔ SmeltUnknown crossing"),
        S("an upstream decision made necessary"),
    ], kind="codegen", href="crates/smelt-codegen-rust/src/emitter/coercion.rs", cls="node panel")
    # arrows from blocks up to their stage (drawn as dashed loss leaders)
    g.path(f"M{st2['x'] + 21},{fy} V{st2['b'] + 2}", cls="edge lossline", marker="arr-loss")
    g.path(f"M{MX + 60},{my} V{st4['b'] + 2}", cls="edge lossline", marker="arr-loss")
    g.path(f"M{MX + MW - 40},{my} V{st5['b'] + 2}", cls="edge lossline", marker="arr-loss")

    end = max(fy + hh + RH * len(rows_f) + 10, cy + 96) + 22
    g.add(f'<rect class="lg loss" x="{lx}" y="{end}" width="14" height="10" rx="2"/>')
    g.add(g.text(lx + 20, end + 10, S("avoidable: the source has a static shape Smelt did not carry")))
    lx2 = lx + 20 + tw("avoidable: the source has a static shape Smelt did not carry", 13, False) + 24
    g.add(f'<rect class="lg legit" x="{lx2:.0f}" y="{end}" width="14" height="10" rx="2"/>')
    g.add(g.text(lx2 + 20, end + 10, S("legitimate boundary: the source type is itself dynamic")))
    g.h = end + 24
    tot = lambda d: sum(v[0] for v in d.values())
    back = lambda d: sum(v[0] for k, v in d.items() if k in BACKEND)
    summary = dict(es_records=ese["records"], es_avoidable=ese["avoidable"], es_backend=back(es), es_total=tot(es),
                   re_records=ree["records"], re_avoidable=ree["avoidable"], re_backend=back(re_), re_total=tot(re_),
                   es=es, re=re_)
    assert summary["es_total"] == ese["records"] and summary["re_total"] == ree["records"]
    return g.render(), summary
