# CLI (`smelt`)

The `smelt` binary is the entry point for every user interaction. It reads `Smelt.toml`, drives the pipeline, and surfaces errors. It owns *no* compilation logic — every phase is a function call into a downstream crate.

## Responsibilities

- Parse command-line args (`clap`, derive API).
- Locate and parse `Smelt.toml` (`config_parser`).
- Validate the manifest (strict-mode required, unknown keys rejected).
- Detect which language pipelines are needed from source extensions.
- Drive the check pipeline per language (see `specs/check-pipeline.md`).
- For `build`: drive HIR → MIR → codegen, optionally invoke `cargo build`.
- Surface every error in `path:line:col: error[code]: message` format.
- Return the right exit code (0 success, 1 user error, 2 internal bug).

## Non-Responsibilities

- Parsing source files. That's the frontends' job.
- Producing HIR or MIR. The CLI calls into `smelt-frontend-*` and `smelt-mir`.
- Type-checking. That's `tsgo` / `ty` via the frontends.
- Caching, daemon mode, watch mode. v2.0+.

## Commands

All commands accept `--manifest-path <path>` (defaults to `./Smelt.toml`).

| Command                  | Purpose                                              | Implemented? |
|--------------------------|------------------------------------------------------|--------------|
| `smelt new <name>`       | Scaffold a new project                               | ✅ done       |
| `smelt build`            | Run check, then HIR → MIR → codegen → emit Rust      | partial      |
| `smelt check`            | Run the check pipeline; do not emit                  | partial      |
| `smelt dump-hir <file>`  | Print HIR for a single source file                   | ✅ done       |
| `smelt dump-mir <file>`  | Print MIR for a single source file                   | ✅ done       |
| `smelt clean`            | Remove `output.target`                               | ✅ done       |
| `smelt dump-schema`      | Print the JSON Schema for `Smelt.toml`               | ✅ done       |

### `smelt new <name>`

Scaffolds the directory:

```
<name>/
├── Smelt.toml
├── .gitignore           # contains "dist/" and ".smelt-cache/"
├── README.md            # one-paragraph stub
└── src/
    └── main.ts          # or main.py if --python
```

`Smelt.toml` is generated from a template with the `name` substituted, default `entries = ["src/main.ts"]` (or `.py`), no `[rust.dependencies]` block.

If the target directory already exists and is non-empty, error.

### `smelt build`

1. Read manifest.
2. Type-check every source: TypeScript with the embedded TypeScript 7 checker, Python with ty, each language as one program. This is the **type gate**: a checker error, or a language with sources that no checker could check (a smelt built without `ts7` or `ty`, `SMELT_TS7=0`, a checker that failed), stops the build with `error[type-check]: …` lines and nothing is written. Smelt's lowering relies on a program that type-checks; `--allow-type-errors` (or `[build] type-errors = "allow"`) lowers it anyway, printing the diagnostics as `warning[type-check]`. A source the checker rejected is then lowered *unchecked*: the frontend shortcuts that rely on a checked program (a typed object spread missing a required field, generic inference meeting unrelated candidates, duplicate function implementations) refuse it with `smelt::ill-typed-source` instead of producing wrong Rust.
3. For each entry source: invoke the corresponding frontend → produces `smelt_hir::Module`s, all merged into one `smelt_hir::Crate`.
4. Run `smelt_hir::validate(&crate)`.
5. Lower HIR → MIR via `smelt_mir::lower(crate)`.
6. Run `smelt_mir::validate(&mir)`.
7. Emit Rust source via `smelt_codegen_rust::emit(mir, &output_dir)`.
8. Generate `Cargo.toml` in `output.target`.
9. If `output.build = true`, shell out to `cargo build` from `output.target` and forward its output.

Stop on the first failing phase; report errors and exit 1.

### `smelt check`

Steps 1–4 from `build`. No MIR, no codegen, no emit. Exit 0 if all phases pass.

Sources are also type-checked as in `build` step 2, and the checkers' errors and warnings are printed as `warning[type-check]: <file>:<line>:<col>: <message>` (`TSnnnn: …` from TypeScript, `<rule>: …` from ty), or as `type-check` rows with `--message-format json`. Unlike `build`, `check` reports them without failing unless `--deny-type-errors` is given: the checker sees only the dependency closure, so a project without `@types/node`, or with ambient test globals, gets errors its own `tsc` setup does not.

When the checker cannot run, lowering proceeds on annotations alone, which changes the output's types, and nothing vouches for the sources: `check` warns that they were not type-checked, and `build` and `check --deny-type-errors` refuse them. `SMELT_TS7=0` turns the TypeScript checker off on purpose, to compare a run with and without its facts; pair it with `--allow-type-errors` to build.

### `smelt dump-hir <file>`

Run the check pipeline only on `<file>` (ignoring entries), produce its HIR, pretty-print to stdout via `smelt_hir::pretty::print`. Useful for debugging frontends.

### `smelt dump-mir <file>`

Same as above but lowers to MIR and pretty-prints.

### `smelt clean`

Remove `output.target` recursively. Prompt for confirmation if it contains files not produced by smelt (heuristic: files without the smelt-generated header). Skip prompt with `--force`.

Produced by smelt: `.rs` files starting with the `@generated` header, `Cargo.toml`, `Cargo.lock`, `smelt-erasure.json` and `smelt-typedness.json` at the top of the target, and Cargo's `target/` inside it. Without a terminal there is no prompt: foreign files make the command fail and name `--force`. A target that is the project directory, or contains it, is refused even with `--force`.

## Manifest Loading

`smelt-transpiler::config_parser::parse(path)` reads, deserializes via `toml`, validates, and returns `Config`. Validation rules:

- `[project].name` and `[project].version` must be present.
- `[sources].entries` must have ≥1 entry; every entry must exist on disk.
- `[strict].typescript` and `[strict].python` must be `true` (or absent → defaults to `true`). Any explicit `false` is a hard error in v1.0.
- `[runtime].clone-strategy` must be `"aggressive"`. Reserved field; other values error.
- Unknown keys are an error, at the top level and inside known tables: a misspelt `[runtime]` would otherwise pass silently, now that the table is optional.

Errors here use the format:

```
Smelt.toml: error[smelt::manifest]: strict.typescript=false is not supported in v1.0
```

## Pipeline Detection

Planned: `Config::pipelines()` should return the set of `Pipeline` variants needed based on entry-file extensions. Pipeline detection by extension is not implemented yet. It should map:

- `.ts`, `.tsx` (rejected by smelt rules) → `Pipeline::TypeScript`
- `.py` → `Pipeline::Python`
- Anything else → error: unsupported source extension

The CLI runs each pipeline independently in v1.0. Cross-language imports are deferred to v1.x.

## Error Format

Every error printed to stderr uses one of:

```
path/to/file.ts:LINE:COL: error[CODE]: MESSAGE
Smelt.toml: error[CODE]: MESSAGE
smelt: error[CODE]: MESSAGE                # CLI-level (no file context)
```

`CODE` is namespaced: `smelt::no-any`, `smelt::manifest`, `smelt::pipeline`, etc.

Exit codes:

- `0` — success
- `1` — user error (manifest, source, type-check, smelt rule, validation)
- `2` — internal bug (validator failure, panic, unreachable)

## Logging

`--verbose` (alias `-v`) enables phase-by-phase status lines on stderr (e.g. `[1/4] oxclint…`). `--quiet` (alias `-q`) suppresses everything except errors and the final summary line.

Telemetry: none. Ever.

## Current Gaps (state of the code → what's missing)

- `smelt-frontend-ts`'s `checker.rs` shells out to `tsc` and `oxlint` via `npx` and has no caller; type errors come from the embedded checker (see `smelt check`).
- No call to `smelt-hir`, `smelt-mir`, or `smelt-codegen-rust` exists from the CLI yet.
- Manifest validator does not enforce the missing-entries check.
- No `--verbose`/`--quiet` flags.
- Error format helper does not exist; phases currently `Box<dyn Error>` and lose source spans.

## TODO (concrete, ordered)

The list below is the v1.0 roadmap for `smelt-cli`, ordered by dependency. Items prefixed `M*` map onto existing milestones.

### Foundation (M0/M1 follow-ups)

- [ ] Add a `SmeltError` type in `smelt-cli` that carries `Option<Span>`, `code: &'static str`, `message: String`. Implement `Display` to produce the canonical format.
- [ ] Replace every `Box<dyn Error>` in `cli.rs`, `config_parser.rs`, and `checker.rs` with `SmeltError`.
- [ ] Set process exit code based on error kind (1 vs 2).
- [ ] Implement `--verbose` / `--quiet` global flags.

### Manifest

- [ ] Validate required keys in `config_parser` and produce `SmeltError`s pointing at `Smelt.toml`.
- [x] Reject `strict.typescript = false` and `strict.python = false`.
- [x] Reject `runtime.clone-strategy != "aggressive"`.
- [x] Reject unknown top-level and per-table keys (`deny_unknown_fields`).
- [ ] Resolve `sources.entries` and `sources.roots` to absolute paths; verify each entry exists.

### Commands

- [x] `smelt check` — reports the embedded TypeScript checker's and ty's diagnostics; `--deny-type-errors` fails on them.
- [x] `smelt build` — refuses a program the type checkers reject; `--allow-type-errors` / `[build] type-errors = "allow"` opts out.
- [ ] `smelt build` — wire HIR → MIR → codegen once those crates exist; invoke `cargo build` if `output.build = true`.
- [x] `smelt dump-hir <file>` — invoke frontend, run validator, call `smelt_hir::pretty::print`.
- [x] `smelt dump-mir <file>` — same, plus lowering.

### Check Pipeline Wiring

- [ ] Replace `todo!("ty")` with a real call into `smelt-frontend-py::check` (which runs the full Python pipeline).
- [ ] Replace the `npx tsc` / `npx oxlint` shells in `checker.rs` with `tsgo --noEmit` and `oxclint`. Look for binaries in `node_modules/.bin` and `$PATH`; error clearly if missing.
- [ ] Surface frontend errors with their original spans via `SmeltError`.

### Output

- [x] Emit a header on every generated `.rs` file: `// @generated by smelt. Do not edit by hand.`.
- [x] Generate `Cargo.toml` from `[project]` + `[rust]` + `[rust.dependencies]` + smelt-detected deps.
- [ ] Add a single integration test that runs `smelt build` on an existing `examples/typescript` fixture and checks the output crate compiles.
