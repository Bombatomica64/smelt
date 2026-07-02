#!/usr/bin/env python3
"""Build the smelt documentation site from the repository's Markdown sources.

Reads the checked-in docs (README, docs/, specs/, devlog/, and the root
reference pages), renders them through the shared template in
site/templates/page.html with the design system in site/assets/, and writes a
static site to _site/ ready for GitHub Pages.

No external site generator: python-markdown + pygments only, so the build is
reproducible in CI with two pip installs.
"""

from __future__ import annotations

import datetime
import html
import re
import shutil
from dataclasses import dataclass, field
from pathlib import Path

import markdown
from pygments.formatters import HtmlFormatter

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "_site"
SITE = REPO / "site"
GITHUB_BLOB = "https://github.com/Bombatomica64/smelt/blob/main/"
GITHUB_RAW = "https://raw.githubusercontent.com/Bombatomica64/smelt/main/"

MD_EXTENSIONS = [
    "extra",
    "codehilite",
    "toc",
    "sane_lists",
]
MD_EXTENSION_CONFIGS = {
    "codehilite": {"guess_lang": False, "css_class": "codehilite"},
    "toc": {"permalink": "#", "permalink_title": "Link to this section"},
}


@dataclass
class Page:
    """One rendered page: where it came from and where it lands."""

    source: Path            # repo-relative markdown source
    out: str                # site-relative output path (e.g. "specs/hir.html")
    section: str            # top-nav section key
    title: str = ""
    group: str = ""         # sidebar group label within the section
    external: bool = False  # pre-built standalone HTML copied verbatim (not templated)


@dataclass
class Section:
    """A top-nav section holding sidebar-grouped pages."""

    key: str
    label: str
    pages: list[Page] = field(default_factory=list)


def read_title(path: Path) -> str:
    """Return the first `# ` heading of a markdown file, else a readable stem.

    Falls back through the first `## ` / setext-style heading before giving up
    and prettifying the filename, so pages that open with a lower heading level
    still get a meaningful sidebar title instead of a raw slug.
    """
    lines = path.read_text(encoding="utf-8").splitlines()
    for line in lines:
        if line.startswith("# "):
            return line[2:].strip().rstrip("#").strip()
    for line in lines:
        if line.startswith("## "):
            return line[3:].strip().rstrip("#").strip()
    return path.stem.replace("-", " ").replace("_", " ").strip().title()


def devlog_title(stem: str) -> str:
    """Turn a dated devlog post filename into a human title.

    Devlog posts are tweet drafts with no headings; their filenames encode the
    date (``YYYY-MM-DD`` with an optional ``-N`` suffix for multiple posts in a
    day), e.g. ``2026-06-22-2`` -> ``June 22, 2026 (part 2)``.
    """
    match = re.match(r"(\d{4})-(\d{2})-(\d{2})(?:-(\d+))?$", stem)
    if not match:
        return stem.replace("-", " ").replace("_", " ").strip().title()
    year, month, day, part = match.groups()
    date = datetime.date(int(year), int(month), int(day))
    label = date.strftime("%B %-d, %Y")
    if part:
        label += f" (part {part})"
    return label


def spec_group(name: str) -> str:
    """Bucket a spec filename into a sidebar group."""
    if re.match(r"m\d+-", name):
        return "Milestones"
    architecture = {
        "architecture", "hir", "cli", "config", "frontend-ts", "frontend-py",
        "stdlib-mapping", "codegen-quality-assessment", "frontend-py-missing-vs-ts",
    }
    if name in architecture:
        return "Architecture"
    return "Plans & investigations"


def milestone_sort_key(page: Page) -> tuple:
    """Sort milestone specs numerically, everything else alphabetically."""
    match = re.match(r"m(\d+)-", page.source.stem)
    if match:
        return (0, int(match.group(1)))
    return (1, page.title.lower())


def collect_pages() -> list[Section]:
    """Discover every markdown source and assign it a section and group."""
    sections = [
        Section("overview", "Overview"),
        Section("docs", "Docs"),
        Section("specs", "Specs"),
        Section("devlog", "Devlog"),
    ]
    by_key = {section.key: section for section in sections}

    overview = [
        ("CONTEXT.md", "Domain vocabulary"),
        ("COMPAT.md", "Compatibility"),
        ("IMPLEMENTATION_CHECKLIST.md", "Implementation checklist"),
        ("Test-TODO.md", "Test TODO"),
    ]
    for name, _label in overview:
        source = REPO / name
        if source.exists():
            by_key["overview"].pages.append(
                Page(source, f"overview/{source.stem.lower().replace('_', '-')}.html", "overview", group="Reference")
            )

    for source in sorted((REPO / "docs").glob("*.md")):
        by_key["docs"].pages.append(Page(source, f"docs/{source.stem}.html", "docs", group="Design docs"))

    for source in sorted((REPO / "specs").glob("*.md")):
        group = spec_group(source.stem)
        by_key["specs"].pages.append(Page(source, f"specs/{source.stem}.html", "specs", group=group))

    devlog_sources = sorted((REPO / "devlog" / "posts").glob("*.md"), reverse=True)
    for source in devlog_sources:
        by_key["devlog"].pages.append(Page(source, f"devlog/{source.stem}.html", "devlog", group="Posts"))
    notes = REPO / "devlog" / "NOTES.md"
    if notes.exists():
        by_key["devlog"].pages.append(Page(notes, "devlog/notes.html", "devlog", group="Notes"))

    # Pre-built standalone HTML pages that live in the repo already styled.
    # They are copied verbatim (not run through the template) and linked from
    # the relevant sidebar group as external-style entries.
    architecture_review = REPO / "docs" / "architecture-review.html"
    if architecture_review.exists():
        by_key["docs"].pages.append(
            Page(architecture_review, "docs/architecture-review.html", "docs",
                 title="Architecture review", group="Design docs", external=True)
        )
    tracker = REPO / "devlog" / "tracker.html"
    if tracker.exists():
        by_key["devlog"].pages.append(
            Page(tracker, "devlog/tracker.html", "devlog",
                 title="Engagement tracker", group="Tracker", external=True)
        )

    for section in sections:
        for page in section.pages:
            if page.external:
                continue
            if section.key == "devlog" and page.group == "Posts":
                page.title = devlog_title(page.source.stem)
            else:
                page.title = read_title(page.source)
        if section.key == "specs":
            grouped: dict[str, list[Page]] = {}
            for page in section.pages:
                grouped.setdefault(page.group, []).append(page)
            ordered = []
            for label in ("Architecture", "Milestones", "Plans & investigations"):
                pages = grouped.get(label, [])
                pages.sort(key=milestone_sort_key if label == "Milestones" else lambda p: p.title.lower())
                ordered.extend(pages)
            section.pages = ordered
    return sections


def rewrite_links(html_text: str, page: Page, all_pages: dict[str, Page]) -> str:
    """Rewrite repo-relative links in rendered markdown to working targets.

    Three cases, applied to both ``href`` and ``src`` attributes:

    * A repo-relative ``.md`` link that has a rendered page -> the local
      ``.html`` output (relative to this page's depth).
    * Any other repo-relative reference (``LICENSE``, ``coverage.svg``,
      files under ``crates/`` or ``blocker-logs/`` that have no rendered page)
      -> an absolute GitHub URL. Links use the ``blob`` view; embedded assets
      (``src``) use the ``raw`` view so images still display.
    * External URLs, in-page fragments, ``mailto:``/``data:`` and paths that
      escape the repo are left untouched.
    """

    def replace(match: re.Match) -> str:
        attr, value = match.group(1), match.group(2)
        if value.startswith(("http://", "https://", "#", "mailto:", "data:")):
            return match.group(0)
        target, _, fragment = value.partition("#")
        if not target:
            return match.group(0)
        source_dir = page.source.parent
        resolved = (source_dir / target).resolve()
        try:
            repo_relative = resolved.relative_to(REPO)
        except ValueError:
            return match.group(0)
        suffix = f"#{fragment}" if fragment else ""
        destination = all_pages.get(str(repo_relative))
        if destination is not None and not destination.external:
            depth = page.out.count("/")
            prefix = "../" * depth
            return f'{attr}="{prefix}{destination.out}{suffix}"'
        base = GITHUB_RAW if attr == "src" else GITHUB_BLOB
        return f'{attr}="{base}{repo_relative}{suffix}"'

    return re.sub(r'(href|src)="([^"]+)"', replace, html_text)


def render_markdown(text: str) -> str:
    """Convert markdown text to HTML with the configured extension set."""
    converter = markdown.Markdown(
        extensions=MD_EXTENSIONS, extension_configs=MD_EXTENSION_CONFIGS
    )
    return converter.convert(text)


def top_nav(active: str, root: str) -> str:
    """Render the header navigation with the active section marked."""
    entries = [
        ("overview", "Overview", "index.html"),
        ("docs", "Docs", "docs/index.html"),
        ("specs", "Specs", "specs/index.html"),
        ("devlog", "Devlog", "devlog/index.html"),
    ]
    parts = []
    for key, label, target in entries:
        current = ' aria-current="true"' if key == active else ""
        parts.append(f'<a href="{root}{target}"{current}>{label}</a>')
    return "".join(parts)


def sidebar_html(section: Section, active: Page | None, root: str) -> str:
    """Render the sidebar for a section, grouped, marking the active page."""
    groups: dict[str, list[Page]] = {}
    for page in section.pages:
        groups.setdefault(page.group, []).append(page)
    parts = []
    for label, pages in groups.items():
        parts.append('<div class="sidebar-group">')
        parts.append(f'<p class="group-label">{html.escape(label)}</p>')
        for page in pages:
            current = ' aria-current="page"' if active is page else ""
            parts.append(
                f'<a href="{root}{page.out}"{current}>{html.escape(page.title)}</a>'
            )
        parts.append("</div>")
    return "\n".join(parts)


def apply_template(template: str, **slots: str) -> str:
    """Fill @@SLOT@@ placeholders in the page template."""
    result = template
    for name, value in slots.items():
        result = result.replace(f"@@{name.upper()}@@", value)
    return result


def build() -> None:
    """Render the whole site into _site/."""
    if OUT.exists():
        shutil.rmtree(OUT)
    (OUT / "assets").mkdir(parents=True)
    for asset in (SITE / "assets").iterdir():
        shutil.copy(asset, OUT / "assets" / asset.name)
    (OUT / ".nojekyll").write_text("")

    dark = HtmlFormatter(style="gruvbox-dark").get_style_defs(".codehilite")
    light = HtmlFormatter(style="gruvbox-light").get_style_defs(".codehilite")
    scoped_light = "\n".join(
        f'[data-theme="light"] {line}' if line and not line.startswith(" ") else line
        for line in light.splitlines()
    )
    (OUT / "assets" / "pygments.css").write_text(f"{dark}\n{scoped_light}\n")

    template = (SITE / "templates" / "page.html").read_text(encoding="utf-8")
    sections = collect_pages()
    all_pages = {
        str(page.source.relative_to(REPO)): page
        for section in sections
        for page in section.pages
    }

    for section in sections:
        for page in section.pages:
            if page.external:
                destination = OUT / page.out
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy(page.source, destination)
                continue
            depth = page.out.count("/")
            root = "../" * depth
            body = render_markdown(page.source.read_text(encoding="utf-8"))
            body = rewrite_links(body, page, all_pages)
            source_path = str(page.source.relative_to(REPO))
            crumbs = (
                f'<a href="{root}index.html">smelt</a><span class="sep">/</span>'
                f'<a href="{root}{section.pages[0].out if section.key != "overview" else "index.html"}">{section.label.lower()}</a>'
                f'<span class="sep">/</span>{html.escape(page.title)}'
            )
            rendered = apply_template(
                template,
                title=html.escape(page.title),
                description=f"smelt documentation — {html.escape(page.title)}",
                root=root,
                topnav=top_nav(section.key, root),
                sidebar=sidebar_html(section, page, root),
                breadcrumbs=crumbs,
                content=body,
                source_note=f"Rendered from <code>{source_path}</code>",
                source_path=source_path,
            )
            destination = OUT / page.out
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text(rendered, encoding="utf-8")

    for section in sections:
        if section.key == "overview" or not section.pages:
            continue
        index = OUT / section.key / "index.html"
        index.write_text(
            f'<!DOCTYPE html><meta charset="utf-8">'
            f'<meta http-equiv="refresh" content="0; url=../{section.pages[0].out}">',
            encoding="utf-8",
        )

    build_landing(template, sections, all_pages)
    total = sum(len(section.pages) for section in sections)
    print(f"built {total} pages -> {OUT}")


def build_landing(template: str, sections: list[Section], all_pages: dict[str, Page]) -> None:
    """Render index.html from the dedicated landing template.

    The landing has its own template (site/templates/landing.html) and its own
    stylesheet with two switchable visual worlds; this function supplies the
    data: pygments-highlighted demo code (the TypeScript input and the Rust
    that smelt actually emits for it), section cards, and live counts.
    """
    from pygments import highlight
    from pygments.lexers import PythonLexer, RustLexer, TypeScriptLexer

    demo_ts = (SITE / "demo" / "input.ts").read_text(encoding="utf-8")
    demo_rs = (SITE / "demo" / "output.rs").read_text(encoding="utf-8")
    formatter = HtmlFormatter(nowrap=False, cssclass="codehilite")
    demo_ts_html = highlight(demo_ts, TypeScriptLexer(), formatter)
    demo_rs_html = highlight(demo_rs, RustLexer(), formatter)
    interop_ts = 'export function add(a: number, b: number): number {\n  return a + b;\n}\n'
    interop_py = 'from math import add\n\nresult: float = add(2.0, 3.0)\nprint(result)\n'
    interop_ts_html = highlight(interop_ts, TypeScriptLexer(), formatter)
    interop_py_html = highlight(interop_py, PythonLexer(), formatter)

    counts = {section.key: len(section.pages) for section in sections}
    total_pages = sum(counts.values())
    cards = f"""
<div class="card-grid">
  <a class="card" href="specs/index.html">
    <h3>Specs<span class="count">{counts.get('specs', 0)}</span></h3>
    <p>Architecture, milestones, and design plans for every compiler stage.</p>
  </a>
  <a class="card" href="docs/index.html">
    <h3>Docs<span class="count">{counts.get('docs', 0)}</span></h3>
    <p>Deep dives: runtime specialization, MIR optimization, metaprogramming.</p>
  </a>
  <a class="card" href="overview/context.html">
    <h3>Vocabulary</h3>
    <p>The load-bearing domain terms used across code and reviews.</p>
  </a>
  <a class="card" href="devlog/index.html">
    <h3>Devlog<span class="count">{counts.get('devlog', 0)}</span></h3>
    <p>Progress notes from the workshop floor.</p>
  </a>
  <a class="card" href="docs/why-smelt.html">
    <h3>Why smelt exists</h3>
    <p>A prod incident, a grudge against decorative types, and the accident that made two languages one crate.</p>
  </a>
</div>"""

    landing_template = (SITE / "templates" / "landing.html").read_text(encoding="utf-8")
    landing = apply_template(
        landing_template,
        demo_ts=demo_ts_html,
        demo_rs=demo_rs_html,
        interop_ts=interop_ts_html,
        interop_py=interop_py_html,
        cards=cards,
        test_count="705+",
        page_count=str(total_pages),
    )
    (OUT / "index.html").write_text(landing, encoding="utf-8")


if __name__ == "__main__":
    build()
