"""Independent checks of docs/architecture/index.html."""
import html.parser, os, re, sys
P = sys.argv[1]
base = os.path.dirname(os.path.abspath(P))
src = open(P).read()
VOID = {"meta", "link", "br", "img", "input", "hr", "col", "source", "wbr", "area", "base", "embed", "param", "track"}
class V(html.parser.HTMLParser):
    def __init__(s):
        super().__init__(); s.stack = []; s.err = []; s.hrefs = []; s.ext = []
    def handle_starttag(s, t, a):
        a = dict(a)
        for k in ("href", "src"):
            if k in a: s.hrefs.append(a[k])
        if t not in VOID: s.stack.append((t, s.getpos()))
    def handle_startendtag(s, t, a):
        a = dict(a)
        if "href" in a: s.hrefs.append(a["href"])
    def handle_endtag(s, t):
        if t in VOID: return
        if not s.stack or s.stack[-1][0] != t:
            s.err.append(f"mismatched </{t}> at {s.getpos()}, open {s.stack[-1] if s.stack else None}"); return
        s.stack.pop()
v = V(); v.feed(src); v.close()
assert not v.err, v.err[:5]
assert not v.stack, v.stack[:5]
bad = []
for h in v.hrefs:
    if h.startswith("#"):
        assert re.search(f'id="{re.escape(h[1:])}"', src), h; continue
    if re.match(r"^[a-z]+:", h): bad.append("external " + h); continue
    if not os.path.exists(os.path.normpath(os.path.join(base, h))): bad.append("missing " + h)
assert not re.search(r'<(script|link|img|iframe)[^>]+(src|href)="https?:', src), "external resource"
# SVG text inside boxes
F = {"m": 0.602, "s": 0.55}
nodes = re.findall(r'<g class="(?:node|tag)[^"]*"[^>]*>(.*?)</g>', src, re.S)
fails = 0; checked = 0
for body in nodes:
    r = re.search(r'<rect x="([\d.]+)" y="([\d.]+)" width="([\d.]+)" height="([\d.]+)"', body)
    x, y, w, h = map(float, r.groups())
    for t in re.finditer(r'<text x="([\d.]+)" y="([\d.]+)" font-size="(\d+)" class="([^"]*)"( text-anchor="(\w+)")?>([^<]*)</text>', body):
        tx, ty, fs = float(t.group(1)), float(t.group(2)), int(t.group(3))
        cls = t.group(4).split(); anchor = t.group(6) or "start"
        text = html.unescape(t.group(7))
        fac = F["m"] if "m" in cls else F["s"]
        if "b" in cls: fac *= 1.06
        tw = len(text) * fs * fac
        x0 = tx - (tw / 2 if anchor == "middle" else tw if anchor == "end" else 0)
        checked += 1
        if not (x0 >= x - 0.5 and x0 + tw <= x + w + 0.5 and ty - fs >= y - 0.5 and ty <= y + h + 0.5):
            fails += 1; print("OUTSIDE", repr(text), (x0, x0 + tw, ty), (x, x + w, y, y + h))
# minimum font size
sizes = sorted(set(int(s) for s in re.findall(r'font-size="(\d+)"', src)))
mins = re.findall(r'--minw:(\d+)px', src); vbs = re.findall(r'viewBox="0 0 (\d+) ', src)
print("parse ok; hrefs", len(v.hrefs), "bad", bad, "; boxed texts", checked, "outside", fails, "; svg font sizes", sizes)
print("min scale per svg:", sorted(set(round(int(m) / int(vb), 2) for m, vb in zip(mins, vbs[3:] if False else vbs[-len(mins):]))))
