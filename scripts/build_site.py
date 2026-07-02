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


@dataclass
class Section:
    """A top-nav section holding sidebar-grouped pages."""

    key: str
    label: str
    pages: list[Page] = field(default_factory=list)


def read_title(path: Path) -> str:
    """Return the first `# ` heading of a markdown file, else its stem."""
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("# "):
            return line[2:].strip().rstrip("#").strip()
    return path.stem.replace("-", " ").replace("_", " ")


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

    for section in sections:
        for page in section.pages:
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
    """Rewrite repo-relative .md hrefs to their rendered .html locations."""

    def replace(match: re.Match) -> str:
        href = match.group(2)
        if href.startswith(("http://", "https://", "#", "mailto:")):
            return match.group(0)
        target, _, fragment = href.partition("#")
        if not target.endswith(".md"):
            return match.group(0)
        source_dir = page.source.parent
        resolved = (source_dir / target).resolve()
        try:
            repo_relative = resolved.relative_to(REPO)
        except ValueError:
            return match.group(0)
        destination = all_pages.get(str(repo_relative))
        if destination is None:
            return match.group(0)
        depth = page.out.count("/")
        prefix = "../" * depth
        suffix = f"#{fragment}" if fragment else ""
        return f'{match.group(1)}{prefix}{destination.out}{suffix}"'

    return re.sub(r'(href=")([^"]+)"', replace, html_text)


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

    build_landing(template, sections)
    total = sum(len(section.pages) for section in sections)
    print(f"built {total} pages -> {OUT}")


def build_landing(template: str, sections: list[Section]) -> None:
    """Render index.html: hero, section cards, then the README body."""
    readme = (REPO / "README.md").read_text(encoding="utf-8")
    readme_body = re.sub(r"^# .*\n", "", readme, count=1)
    body = render_markdown(readme_body)

    counts = {section.key: len(section.pages) for section in sections}
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
</div>"""

    hero = f"""
<section class="hero">
  <p class="kicker">typescript · python → rust</p>
  <h1>smelt</h1>
  <p class="tagline">Smelt your strictly-typed TypeScript and Python down to idiomatic Rust — one shared IR, one generated crate, your tests carried across.</p>
  <div class="pipeline">
    <span class="stage lang-ts">.ts</span>
    <span class="stage lang-py">.py</span>
    <span class="arrow">→</span>
    <span class="stage">HIR</span>
    <span class="arrow">→</span>
    <span class="stage">MIR</span>
    <span class="arrow">→</span>
    <span class="stage lang-rs">.rs</span>
  </div>
</section>
{cards}
<div class="landing-body">
{body}
</div>"""

    overview = next(section for section in sections if section.key == "overview")
    landing = apply_template(
        template,
        title="smelt — TypeScript & Python to Rust",
        description="smelt transpiles strictly-typed TypeScript and Python into idiomatic Rust.",
        root="",
        topnav=top_nav("overview", ""),
        sidebar=sidebar_html(overview, None, ""),
        breadcrumbs='<a href="index.html">smelt</a>',
        content=hero,
        source_note="Rendered from <code>README.md</code>",
        source_path="README.md",
    )
    landing = landing.replace("<body>", '<body class="landing">')
    (OUT / "index.html").write_text(landing, encoding="utf-8")


if __name__ == "__main__":
    build()
