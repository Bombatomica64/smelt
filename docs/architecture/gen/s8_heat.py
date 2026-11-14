"""Section 8: churn (60 days) against size."""
import json, math
from svg import Svg, L, H, S, esc, tw

def kind(m):
    if "frontend-ts" in m or "frontend-py" in m: return "front"
    if "codegen-rust" in m: return "codegen"
    return "other"

HOT = {"crates/smelt-frontend-ts/src/lowering/stdlib": 1,
       "crates/smelt-codegen-rust/src/emitter/coercion": 2,
       "crates/smelt-frontend-ts/src/lowering/expr": 3}

ALIAS = {"frontend-ts": "ts", "frontend-py": "py", "codegen-rust": "cg", "transpiler": "tr", "runtime": "rt"}


def short(m):
    p = m.split("/")
    crate = p[1].replace("smelt-", "")
    rest = "/".join(p[3:])
    return f"{crate}::{rest}"


def label(m):
    p = m.split("/")
    crate = p[1].replace("smelt-", "")
    return f"{ALIAS.get(crate, crate)} " + "/".join(p[3:])

def build():
    d = json.load(open(__import__("os").path.join(__import__("os").path.dirname(__import__("os").path.abspath(__file__)), "churn.json")))
    pts = [r for r in d if r["lines"] > 0 and "/src/" in r["module"] + "/" and not r["module"].endswith("/tests")
           and "/src/tests" not in r["module"] and r["module"].count("/") >= 3]
    W, Hh = 1040, 600
    g = Svg(W, Hh, "Commits in the last 60 days against current lines, per module")
    X0, X1, Y0, Y1 = 80, 1010, 30, 520
    lx = lambda n: X0 + (math.log10(max(n, 10)) - math.log10(10)) / (math.log10(40000) - math.log10(10)) * (X1 - X0)
    YMAX = 140
    ly = lambda c: Y1 - c / YMAX * (Y1 - Y0)
    # quadrant shading: >= 3000 lines and >= 40 commits
    g.add(f'<rect class="hotzone" x="{lx(3000):.1f}" y="{ly(YMAX):.1f}" width="{X1 - lx(3000):.1f}" height="{ly(40) - ly(YMAX):.1f}"/>')
    for v in (0, 20, 40, 60, 80, 100, 120, 140):
        g.add(f'<line class="grid" x1="{X0}" x2="{X1}" y1="{ly(v):.1f}" y2="{ly(v):.1f}"/>')
        g.add(g.text(X0 - 8, ly(v) + 4, L(str(v), cls="t2"), anchor="end"))
    for v in (10, 30, 100, 300, 1000, 3000, 10000, 30000):
        g.add(f'<line class="grid" y1="{Y0}" y2="{Y1}" x1="{lx(v):.1f}" x2="{lx(v):.1f}"/>')
        lab = f"{v // 1000}k" if v >= 1000 else str(v)
        g.add(g.text(lx(v), Y1 + 20, L(lab, cls="t2"), anchor="middle"))
    g.add(g.text((X0 + X1) / 2, Y1 + 44, S("lines of Rust in the module today (log scale)"), anchor="middle"))
    g.add(f'<text class="s t2" font-size="13" transform="translate(22 {(Y0 + Y1) / 2}) rotate(-90)" text-anchor="middle">non-merge commits touching it, 60 days</text>')
    placed = [(X0, Y0, X0 + 760, Y0 + 28)]
    def free(b):
        for p in placed:
            if not (b[2] < p[0] or b[0] > p[2] or b[3] < p[1] or b[1] > p[3]):
                return False
        return X0 <= b[0] and b[2] <= X1 and Y0 <= b[1] and b[3] <= Y1
    pts.sort(key=lambda r: (r["commits"], r["lines"]))
    for r in pts:
        x, y = lx(r["lines"]), ly(min(r["commits"], YMAX))
        rr = 12 if r["module"] in HOT else 5
        placed.append((x - rr, y - rr, x + rr, y + rr))
    for r in pts:
        x, y = lx(r["lines"]), ly(min(r["commits"], YMAX))
        g.add(f'<circle class="pt k-{kind(r["module"])}" cx="{x:.1f}" cy="{y:.1f}" r="5"><title>{esc(short(r["module"]))}: {r["lines"]} lines, {r["commits"]} commits, {r["changed"]} lines changed</title></circle>')
    for r in pts:
        if r["module"] in HOT:
            x, y = lx(r["lines"]), ly(min(r["commits"], YMAX))
            t = f"{HOT[r['module']]}  {label(r['module'])}"
            w = tw(t, 13)
            for bx, by, anc in ((x - 16, y + 5, "end"), (x + 16, y + 5, "start"), (x - w / 2, y + 30, "start"), (x - w / 2, y - 16, "start"), (x - w + 10, y + 30, "start")):
                b0 = bx if anc == "start" else bx - w
                box = (b0 - 1, by - 12, b0 + w + 1, by + 3)
                if free(box):
                    placed.append(box)
                    g.add(g.text(bx, by, L(t, bold=True), anchor=anc))
                    break
            else:
                raise SystemExit("no room for hot label " + t)
    labelled = [r for r in pts if (r["commits"] >= 40 or r["lines"] >= 5000) and r["module"] not in HOT]
    labelled.sort(key=lambda r: -r["commits"])
    for r in labelled:
        x, y = lx(r["lines"]), ly(min(r["commits"], YMAX))
        t = label(r["module"])
        w = tw(t, 13)
        for bx, by, anc in ((x + 9, y + 4, "start"), (x - 9, y + 4, "end"), (x - w / 2, y - 10, "start"), (x - w / 2, y + 19, "start")):
            b0 = bx if anc == "start" else bx - w
            box = (b0 - 1, by - 12, b0 + w + 1, by + 3)
            if free(box):
                placed.append(box)
                g.add(g.text(bx, by, L(t), anchor=anc))
                break
    for m, n in HOT.items():
        r = next(r for r in pts if r["module"] == m)
        x, y = lx(r["lines"]), ly(min(r["commits"], YMAX))
        g.add(f'<circle class="hotring" cx="{x:.1f}" cy="{y:.1f}" r="11"/>')
    # legend
    lgx = X0 + 10
    for k, lab in (("front", "frontends: ts, py"), ("codegen", "cg: codegen-rust"), ("other", "mir, hir, rt: runtime, tr: transpiler, others")):
        g.add(f'<circle class="pt k-{k}" cx="{lgx + 5}" cy="{Y0 + 14}" r="5"/>')
        g.add(g.text(lgx + 16, Y0 + 19, S(lab)))
        lgx += 26 + tw(lab, 13, False)
    g.add(f'<rect class="hotzone" x="{lgx}" y="{Y0 + 8}" width="14" height="12"/>')
    g.add(g.text(lgx + 20, Y0 + 19, S("≥ 3k lines and ≥ 40 commits")))
    assert lgx + 20 + tw("≥ 3k lines and ≥ 40 commits", 13, False) < X1
    return g.render(), pts
