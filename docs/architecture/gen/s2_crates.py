"""Section 2: crate dependency graph, orthogonally routed."""
import json, subprocess, collections
from svg import Svg, L, H, S, esc

ROOT = __import__("subprocess").check_output(["git", "rev-parse", "--show-toplevel"], cwd=__import__("os").path.dirname(__import__("os").path.abspath(__file__))).decode().strip()


def metadata():
    out = subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT)
    m = json.loads(out)
    names = {p["name"] for p in m["packages"]}
    deps = []
    for p in m["packages"]:
        for d in p["dependencies"]:
            if d["name"] in names:
                deps.append((p["name"], d["name"], d.get("kind") or "normal", d.get("optional"), d.get("uses_default_features")))
    return m, deps


def src_lines(crate, sub="src"):
    files = [f for f in subprocess.check_output(["git", "ls-files", f"crates/{crate}"], cwd=ROOT).decode().split() if f.endswith(".rs")]
    s = o = 0
    for f in files:
        n = sum(1 for _ in open(f"{ROOT}/{f}", errors="ignore"))
        if f.startswith(f"crates/{crate}/src/"):
            s += n
        else:
            o += n
    return s, o


KIND = {"smelt-transpiler": "driver", "smelt-gui": "driver", "smelt-frontend-ts": "front", "smelt-frontend-py": "front",
        "smelt-codegen-rust": "codegen", "smelt-mir": "mir", "smelt-hir": "hir", "smelt-runtime": "runtime",
        "smelt-stdlib": "support", "smelt-specialize": "support", "smelt-ts-types": "check", "smelt-ts-model": "check",
        "smelt-py-types": "check"}
POS = {  # (layer, column)
    "smelt-transpiler": (0, 2), "smelt-gui": (0, 4),
    "smelt-ts-types": (1, 0), "smelt-frontend-ts": (1, 1), "smelt-frontend-py": (1, 2), "smelt-codegen-rust": (1, 3),
    "smelt-ts-model": (2, 0), "smelt-specialize": (2, 1), "smelt-py-types": (2, 2), "smelt-mir": (2, 3),
    "smelt-stdlib": (3, 1), "smelt-hir": (3, 2), "smelt-runtime": (3, 3),
}
FEATURE = {("smelt-transpiler", "smelt-ts-types"): "ts7", ("smelt-transpiler", "smelt-frontend-py"): "python",
           ("smelt-frontend-py", "smelt-py-types"): "ty"}


def build():
    m, deps = metadata()
    normal = [(a, b, opt, udf) for a, b, k, opt, udf in deps if k == "normal"]
    dev = [(a, b) for a, b, k, opt, udf in deps if k == "dev"]
    adj = collections.defaultdict(set)
    for a, b, *_ in normal:
        adj[a].add(b)

    def reach(a, b, skip):
        seen, st = set(), [a]
        while st:
            x = st.pop()
            for y in adj[x]:
                if (x, y) == skip or y in seen:
                    continue
                if y == b:
                    return True
                seen.add(y)
                st.append(y)
        return False

    lines = {c: src_lines(c) for c in POS}
    W, BW, BH = 1040, 180, 92
    LY = [40, 280, 520, 760]
    colx = lambda i: 16 + i * 210
    g = Svg(W, 872, "Workspace crate dependency graph")
    box = {}
    maxl = max(s for s, o in lines.values())
    for c, (ly, col) in POS.items():
        s, o = lines[c]
        x, y = colx(col), LY[ly]
        ls = [L(c, bold=True, size=14), L(f"{s / 1000:.1f}k lines in src/")]
        if o > 500:
            ls.append(L(f"+{o / 1000:.1f}k in tests/", cls="t2"))
        nd = g.node(x, y, BW, BH, ls, kind=KIND[c],
                    href=f"crates/{c}/Cargo.toml", nid=c, dashed=(c == "smelt-gui"), cls="node crate",
                    title=f"{c}: {s} lines of Rust under src/ (git-tracked), {o} more under tests/ and other dirs")
        bw = max(2, (BW - 20) * s / maxl)
        g.add(f'<rect class="bar k-{KIND[c]}" x="{x + 10}" y="{y + BH - 18}" width="{bw:.1f}" height="8" rx="2"><title>{s} lines</title></rect>')
        box[c] = nd
    # route
    edges = [(a, b, opt, udf) for a, b, opt, udf in normal if a in POS and b in POS]
    out_by = collections.defaultdict(list)
    in_by = collections.defaultdict(list)
    for e in edges:
        out_by[e[0]].append(e)
        in_by[e[1]].append(e)
    ex, nx = {}, {}
    for c, es in out_by.items():
        es.sort(key=lambda e: (box[e[1]]["cx"], -POS[e[1]][0]))
        n = len(es)
        for i, e in enumerate(es):
            ex[e] = box[c]["x"] + 31 + (BW - 48) * (i + 0.5) / n
    for c, es in in_by.items():
        es.sort(key=lambda e: (box[e[0]]["cx"], POS[e[0]][0]))
        n = len(es)
        for i, e in enumerate(es):
            nx[e] = box[c]["x"] + 24 + (BW - 48) * (i + 0.5) / n
    track = collections.Counter()
    gtrack = collections.Counter()
    tagq = []
    # longer edges get the upper tracks
    order = sorted(edges, key=lambda e: (POS[e[0]][0], -abs(ex[e] - nx[e])))
    for e in order:
        a, b, opt, udf = e
        la, lb = POS[a][0], POS[b][0]
        cls = ["edge", "dep"]
        if reach(a, b, (a, b)):
            cls.append("redundant")
        if opt:
            cls.append("opt")
        sx, sy = ex[e], LY[la] + BH
        if la == lb:  # same layer: side to side
            y = LY[la] + BH / 2
            if box[a]["x"] > box[b]["x"]:
                d = f"M{box[a]['x']},{y} H{box[b]['r'] + 2}"
            else:
                d = f"M{box[a]['r']},{y} H{box[b]['x'] - 2}"
        else:
            t1 = track[la]; track[la] += 1
            y1 = LY[la] + BH + 16 + t1 * 7
            tx, ty = nx[e], LY[lb]
            if lb == la + 1:
                d = f"M{sx:.1f},{sy} V{y1} H{tx:.1f} V{ty - 2}"
            else:
                tcol = POS[b][1]
                gap = tcol if box[a]["cx"] < box[b]["cx"] else tcol + 1  # gap index: left of col i is gap i
                if box[a]["cx"] == box[b]["cx"]:
                    gap = tcol + 1
                k = gtrack[gap]; gtrack[gap] += 1
                cx = colx(gap) - 15 + (-5, 5, 0, -10, 10)[k % 5]
                t2 = track[lb - 1]; track[lb - 1] += 1
                y2 = LY[lb - 1] + BH + 16 + t2 * 7
                d = f"M{sx:.1f},{sy} V{y1} H{cx:.1f} V{y2} H{tx:.1f} V{ty - 2}"
        g.add(f'<path d="{d}" class="{" ".join(cls)}" data-from="{a}" data-to="{b}" marker-end="url(#arr)"><title>{esc(a)} → {esc(b)}</title></path>')
        lab = FEATURE.get((a, b))
        if b == "smelt-runtime":
            lab = "default-features = false"
        if lab:
            tagq.append((nx[e], LY[lb] - 13, lab, opt))
    for x, y, lab, opt in tagq:
        g.tag(x, y, ("feature " if opt else "") + lab, kind="feat")
    assert max(track.values()) * 7 + 16 < (LY[1] - LY[0] - BH - 6), track
    # feature tags beside the source exit
    return g, edges, dev, lines, tagq


def render():
    g, edges, dev, lines, tagq = build()
    return g, edges, dev, lines
