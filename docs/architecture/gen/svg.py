"""Tiny SVG builder with text-fit checking for the architecture page."""
import html

MONO = 0.61   # em width per char, monospace (DejaVu Sans Mono 0.602, Menlo 0.602)
SANS = 0.57   # conservative average for system sans
SANS_B = 0.62

REPO_PREFIX = "../../"
LINKS = set()


def esc(s):
    return html.escape(str(s), quote=True)


def tw(text, size, mono=True, bold=False):
    f = MONO if mono else (SANS_B if bold else SANS)
    if bold and mono:
        f = 0.63
    return len(text) * size * f


class FitError(Exception):
    pass


def link(path):
    """Relative link from docs/architecture/ to a repo path; recorded for checking."""
    LINKS.add(path)
    return REPO_PREFIX + path


class Line:
    def __init__(self, text, size=13, mono=True, bold=False, cls="", href=None):
        self.text, self.size, self.mono, self.bold, self.cls, self.href = text, size, mono, bold, cls, href

    @property
    def width(self):
        return tw(self.text, self.size, self.mono, self.bold)


def L(text, **kw):
    return Line(text, **kw)


def H(text, size=14):
    return Line(text, size=size, mono=False, bold=True)


def S(text, size=13):
    return Line(text, size=size, mono=False)


class Svg:
    def __init__(self, w, h, label):
        self.w, self.h, self.label = w, h, label
        self.parts = []
        self.nodes = []

    def add(self, s):
        self.parts.append(s)

    def text(self, x, y, line, anchor="start", extra=""):
        cls = ["m" if line.mono else "s"]
        if line.bold:
            cls.append("b")
        if line.cls:
            cls.append(line.cls)
        t = (f'<text x="{x:.1f}" y="{y:.1f}" font-size="{line.size}" class="{" ".join(cls)}"'
             f'{" text-anchor=" + chr(34) + anchor + chr(34) if anchor != "start" else ""}{extra}>{esc(line.text)}</text>')
        if line.href:
            t = f'<a href="{esc(link(line.href))}">{t}</a>'
        return t

    def node(self, x, y, w, h, lines, kind="driver", href=None, title=None, pad=10,
             gap=4, center=False, dashed=False, nid=None, cls="node", rx=6, valign="top"):
        """A box with lines of text inside; raises FitError if any line overflows."""
        need_h = pad * 2 + sum(l.size for l in lines) + gap * (len(lines) - 1) + 3
        if need_h > h + 0.5:
            raise FitError(f"node {lines[0].text!r}: needs h={need_h:.0f} > {h}")
        for l in lines:
            if l.width > w - 2 * pad + 0.5:
                raise FitError(f"text {l.text!r} ({l.width:.0f}px) overflows box w={w} in {lines[0].text!r}")
        g = [f'<g class="{cls} k-{kind}{" dashed" if dashed else ""}"' + (f' data-id="{nid}"' if nid else "") + ">"]
        if title:
            g.append(f"<title>{esc(title)}</title>")
        g.append(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}"/>')
        cy = y + pad
        if valign == "middle":
            cy = y + (h - (sum(l.size for l in lines) + gap * (len(lines) - 1))) / 2
        for l in lines:
            cy += l.size
            if center:
                g.append(self.text(x + w / 2, cy - 2, l, anchor="middle"))
            else:
                g.append(self.text(x + pad, cy - 2, l))
            cy += gap
        g.append("</g>")
        s = "".join(g)
        if href:
            s = f'<a href="{esc(link(href))}">{s}</a>'
        self.add(s)
        self.nodes.append((x, y, w, h))
        return dict(x=x, y=y, w=w, h=h, cx=x + w / 2, cy=y + h / 2, b=y + h, r=x + w)

    def tag(self, x, y, text, size=13, kind="tag", anchor="middle", mono=True):
        """A small label with a background box, centred on (x, y)."""
        line = Line(text, size=size, mono=mono)
        w = line.width + 10
        h = size + 8
        x0 = x - w / 2 if anchor == "middle" else x
        self.add(f'<g class="tag k-{kind}"><rect x="{x0:.1f}" y="{y - h / 2:.1f}" width="{w:.1f}" height="{h}" rx="4"/>'
                 + self.text(x0 + w / 2, y + size / 2 - 2, line, anchor="middle") + "</g>")
        return dict(x=x0, y=y - h / 2, w=w, h=h)

    def path(self, d, cls="edge", marker="arr", title=None):
        m = f' marker-end="url(#{marker})"' if marker else ""
        t = f"<title>{esc(title)}</title>" if title else ""
        self.add(f'<path d="{d}" class="{cls}"{m}>{t}</path>')

    def render(self, minw_factor=0.93):
        defs = ''
        _unused = ('<defs>'
                '<marker id="arr" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">'
                '<path d="M0,0 L10,5 L0,10 z" class="arrowhead"/></marker>'
                '<marker id="arr-loss" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">'
                '<path d="M0,0 L10,5 L0,10 z" class="arrowhead-loss"/></marker>'
                '<marker id="arr-good" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">'
                '<path d="M0,0 L10,5 L0,10 z" class="arrowhead-good"/></marker>'
                '</defs>')
        # marker ids must be unique per document: suffix them
        body = "".join(self.parts)
        return (f'<svg viewBox="0 0 {self.w} {self.h}" role="img" aria-label="{esc(self.label)}" '
                f'style="--minw:{int(self.w * minw_factor)}px;--maxw:{int(self.w * 1.12)}px" '
                f'xmlns="http://www.w3.org/2000/svg">{defs}{body}</svg>')
