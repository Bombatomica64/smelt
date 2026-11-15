# blocker-logs

This directory holds three kinds of file:

- **CI baselines**: the JSON marks the regression gates compare against.
- **Methodology**: how each baseline's metric is measured.
- **Current-status reports**: the present state of a campaign that README.md, CI or a compat
  manifest still points at.

Investigation reports also land here while their investigation is open. They are temporary (see
[Investigation reports](#investigation-reports)).

## Live files

"Full mode" means the gate runs on every push to `main`, the nightly schedule, a manual dispatch,
and pull requests labelled `run-regressions` (see the banner above `remeda-regression` in
`.github/workflows/ci.yml`).

### CI baselines

| File | What writes it | What reads it | Gates CI |
| --- | --- | --- | --- |
| `clippy-baseline.json` | `scripts/check_clippy_ratchet.py --update`, when a count fell | `ci.yml` `build` job, clippy step (`check_clippy_ratchet.py`) | blocking on every run: a count may only fall |
| `smelt-unknown-baseline.json` | `smelt smelt-unknown-report examples --baseline … --update-baseline` | `ci.yml` `corpus` job, "SmeltUnknown examples invariant" | blocking (`--fail-on-regression`), full mode; avoidable must stay 0 |
| `smelt-unknown-baseline-es-toolkit.json` | `smelt smelt-unknown-report <es-toolkit dist-smelt/src> --baseline … --update-baseline` | `ci.yml` `es-toolkit-regression` | blocking (`--fail-on-regression`), full mode |
| `smelt-unknown-baseline-remeda.json` | same command over the Remeda crate | `ci.yml` `remeda-regression` | advisory (`\|\| true`); a missing file fails |
| `smelt-unknown-baseline-hono.json` | same command over the Hono crate | `ci.yml` hono job | advisory (`\|\| true`); a missing file fails |
| `typedness-baseline.json` | `smelt typedness-report … --update-baseline` | `ci.yml` `corpus` job, "Typedness examples ratchet" | blocking (`--fail-on-regression`), full mode |
| `typedness-baseline-es-toolkit.json` | same, over the es-toolkit `dist-smelt` | `ci.yml` `es-toolkit-regression` | blocking (`--fail-on-regression`), full mode |
| `typedness-baseline-remeda.json` | same, over the Remeda `dist-smelt` | `ci.yml` `remeda-regression` | advisory (`\|\| true`) |
| `es-toolkit-ci-baseline.json` | edited by hand when a mark moves (see its `_comment`) | `scripts/check_library_gate.py` in `es-toolkit-regression` | blocking, full mode |
| `hono-ci-baseline.json` | edited by hand when a mark moves (see its `_comment`) | `scripts/check_library_gate.py` in the hono job | blocking, full mode |

Baseline JSON is only ever rewritten by the tool that owns it, in the commit that moves the number.
`AGENTS.md` (`## SmeltUnknown enforcement`, `## Typedness ratchet`) holds the rules for doing that.

### Methodology and generated reports

| File | What writes it | What reads it | Gates CI |
| --- | --- | --- | --- |
| `smelt-unknown-report.md` | by hand | AGENTS.md, README.md: how the SmeltUnknown metric is counted | no |
| `typedness-report.md` | by hand | AGENTS.md, README.md: how the typedness metric is counted | no |
| `ts7-parity.md` | by hand, from `smelt ts7-parity-report` | README.md (`ts7-parity-report` row): how type parity with the TypeScript 7 checker is measured | no |
| `library-probes.md` | `.github/workflows/library-probes.yml` (nightly; commits it) through `scripts/probe_libraries.py` | README.md, `.github/compat/README.md` | no |

### Current-status reports

Each of these is still cited as the current state of something. Delete it, and fix its citations,
when that something closes.

| File | Cited by | Describes |
| --- | --- | --- |
| `hono-current.md` | `ci.yml` hono job comments, `.github/compat/hono/Smelt.toml` | the Hono probe: what builds, what the test run gives |
| `hono-campaign-plan.md` | `ci.yml` hono job comment, `examples/typescript/hono_crud/README.md`, `render_scope.rs` | the Hono campaign plan, its scope rules and its DEFERRED list |
| `hono-phase3-round1-families.md` | `.github/compat/hono/Smelt.toml` | the blocker families behind the manifest's "PHASE 3 PENDING" excludes |
| `hono-fetch-demand.md` | `.github/compat/hono/Smelt.toml`, several code comments | Hono's use of fetch, streams, crypto: why the streaming, websocket and jwt modules are excluded |
| `hono-crud-status.md` | README.md, `benchmarks/hono_crud/`, `examples/typescript/hono_crud/README.md` | the `hono_crud` showcase: what breaks, and how its speed compares with Node |
| `hono-crud-python-app.md` | README.md | the design and cost of a Python app over the TypeScript Hono library |
| `express-v1-baseline.md` | README.md, `host_modules.rs`, `host_module_tests.rs` | what of Express transpiles today |
| `globals-suite-failures.md` | README.md, `ci.yml` comments | the Vitest-globals failure families, including the `Proxy` traps still out of reach |
| `probe-date-fns.md` | `.github/compat/README.md`, `.github/compat/date-fns/Smelt.toml` | the first scoping probe of date-fns |
| `probe-cachetools.md` | `.github/compat/README.md` | the first scoping probe of cachetools |
| `OPEN-QUESTIONS.md` | this file | unresolved problems found in reports that have since been deleted |

## Investigation reports

`AGENTS.md` sends `smelt rust-diagnostics`, `smelt rust-test-report` and debugging write-ups to
`blocker-logs/<name>.md`. Such a report is temporary:

- `.gitignore` ignores `blocker-logs/*.md` except the files above, so a report stays local until
  you commit it with `git add -f`. Commit it only when another session or reviewer needs it.
- Delete the report in the PR that closes the investigation. Conclusions that last (a design
  decision, a rule, a known limitation) go to `docs/`, `specs/`, or a doc comment at the code they
  explain. An open problem with no other home goes to `OPEN-QUESTIONS.md`.
- Do not point a code comment at a report. Write the reason in the comment itself.

Code and docs that cited a deleted report now name it as `` `name.md` (in git history) ``. A few
cite reports that were never committed; they read `(never committed)`, and there is nothing to
recover for them.

## Recovering a deleted report

```sh
git log --diff-filter=D --name-only --format='%h %cs %s' -- blocker-logs/   # every deleted report and the commit that deleted it
git log --all -1 --format=%h -- blocker-logs/<name>.md                      # last commit that touched one report
git show <commit>^:blocker-logs/<name>.md                                    # its content, if <commit> deleted it
```
