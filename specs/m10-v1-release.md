# M10: v1.0 Polish & Release

**Milestone:** v1.0
**Estimated duration:** 3–4 weeks
**Depends on:** M9

## Goal

Make smelt usable by someone who is not the author. Ship v1.0.0.

## Why this matters

Up to this point, smelt has been a personal project tested by its own author. M10 is where it becomes a tool other people can pick up, run, and have a reasonable first experience with. This milestone is unglamorous and easy to skip — don't.

## Scope

### CLI polish

- `smelt new <n>` scaffolds a new project with `Smelt.toml`, `src/main.ts` (or `--python`), `.gitignore`, and a README.
- `smelt build`, `smelt check`, `smelt clean`, `smelt dump-hir`, `smelt dump-mir` all behave consistently.
- `--help` text on every subcommand.
- `--version` shows the smelt version.
- Exit codes follow Unix conventions (0 success, non-zero failure, distinct codes for different failure classes documented in the README).

### Error messages

Every error message includes:
- File path, line, column in `path:line:col` format
- A short description of what went wrong
- Where possible, a suggestion for how to fix it

The error format is recognized by every editor's "click to jump" feature, which gives us 60% of LSP value with 1% of the effort. (Reminder: there is no LSP planned.)

A "common errors" page in the docs lists the top ~20 errors users will hit and how to resolve them.

### Documentation site

Plain markdown is fine; mdBook or a simple static site generator is fine. No fancy frameworks. Contents:

- **Getting started.** Install, `smelt new`, `smelt build`, run the binary.
- **The supported subset.** Brutally honest list of what works for TS and Python.
- **The unsupported subset.** Equally brutal list of what doesn't, with reasons.
- **Configuration reference.** The full `Smelt.toml` schema (lifted from `specs/config.md`).
- **Architecture overview.** A friendlier version of `specs/architecture.md` for users who want to understand what's happening under the hood.
- **The Express demo walkthrough.** Show input, output, and explain the mapping.
- **The FastAPI demo walkthrough.** Same.
- **FAQ.** "Why does it clone everything?" "Why no LSP?" "Will it ever support `any`?" etc.
- **Roadmap.** What's coming in v1.1, v2.0.
- **Testing strategy.** Explain how source-language tests will lower to native Rust tests over time; summarize `specs/testing-strategy.md`.

### Examples

- `examples/express-demo/` (from M7)
- `examples/fastapi-demo/` (from M9)
- `examples/hello-world-ts/` — simplest possible TS project
- `examples/hello-world-py/` — simplest possible Python project
- `examples/sync-script-ts/` — non-web TS that does some computation

Each example has its own README explaining what it demonstrates.

### Release engineering

- Tagged release `v1.0.0` on GitHub.
- Pre-built binaries for x86_64-linux, x86_64-darwin, aarch64-darwin, x86_64-windows. (CI builds them; release uploads them.)
- A homebrew formula or install script if time permits — optional but nice.
- Crates.io publish for the user-facing crates (`smelt-cli` at minimum).
- An announcement post or README "what is this" pitch good enough to send to friends.

## Exit Criteria

- [ ] A new user can run `smelt new my-app && cd my-app && smelt build && ./dist/target/debug/my_app` and see it work.
- [ ] All five example projects build and run.
- [ ] Documentation site is published (GitHub Pages is fine).
- [ ] Pre-built binaries are attached to the v1.0.0 GitHub release.
- [ ] The "supported subset" doc lists every feature with a green check.
- [ ] The "unsupported subset" doc lists every known limitation honestly.
- [ ] CI is green on the release commit.
- [ ] The docs explain the difference between current snapshot/runtime tests and future `smelt test` source-test lowering.

## Out of Scope

- Marketing beyond a single announcement post.
- Tutorial videos.
- A package manager / registry.
- Anything that should have been in M0–M9 but got cut.

## Notes

The honesty in the "supported / unsupported" docs is non-negotiable. Overpromising will burn early users and kill the project. Underpromising is fine — people are happy when something exceeds expectations and angry when it doesn't meet them. List less, deliver more.

After this ships, take a break before starting v1.1. Year-long projects need recovery time.
