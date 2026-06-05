# Configuration: `Smelt.toml`

smelt projects are configured via a `Smelt.toml` manifest at the project root, modeled after `Cargo.toml`. The CLI commands (`smelt build`, `smelt check`, `smelt clean`) operate on the current directory's manifest by default.

## Rationale

Passing source files on the command line (`smelt build app.ts`) does not scale beyond toy examples. A manifest gives us:

- A canonical project root.
- A place to declare entry points and let smelt discover the rest via imports.
- A place to pin Rust dependency versions for the generated `Cargo.toml`.
- A place to configure strictness, output paths, and (eventually) optimization levels.
- A familiar mental model for anyone who has used Cargo, npm, or Poetry.

## Example

```toml
[project]
name = "my-app"
version = "0.1.0"
description = "An Express app smelted into Rust"

[sources]
# Entry points. smelt walks imports from here to discover the rest.
entries = ["src/main.ts"]
# Optional: extra roots searched during import resolution.
roots = ["src", "lib"]

[output]
target = "./dist"
crate-name = "my_app"
# "program" emits src/main.rs; "library" emits src/lib.rs.
kind = "program"
# If true, smelt also runs `cargo build` after emitting.
build = false

[rust]
edition = "2024"

[rust.dependencies]
axum = "0.7"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[strict]
# v1.0: both flags must be true. Non-strict mode is not supported.
typescript = true
python = true

[runtime]
# v1.0 only supports "aggressive". Reserved for v2.0.
clone-strategy = "aggressive"
```

## Section Reference

### `[project]`

| Key           | Type   | Required | Description                              |
| ------------- | ------ | -------- | ---------------------------------------- |
| `name`        | string | yes      | Project name. Used as default crate-name |
| `version`     | string | yes      | Semver. Propagated to generated Cargo.toml |
| `description` | string | no       | Free-form                                |

### `[sources]`

| Key       | Type            | Required | Description                                              |
| --------- | --------------- | -------- | -------------------------------------------------------- |
| `entries` | array of string | yes      | At least one. Paths relative to manifest                 |
| `roots`   | array of string | no       | Extra directories searched when resolving bare imports   |

### `[output]`

| Key          | Type   | Default              | Description                                      |
| ------------ | ------ | -------------------- | ------------------------------------------------ |
| `target`     | string | `./dist`             | Where to emit the generated Rust crate           |
| `crate-name` | string | `project.name` (snake_cased) | Name of the generated Rust crate         |
| `kind`       | string | `"program"`          | Output target kind: `"program"` or `"library"`   |
| `build`      | bool   | `false`              | If true, run `cargo build` after emitting        |

### `[rust]`

| Key            | Type   | Default  | Description                              |
| -------------- | ------ | -------- | ---------------------------------------- |
| `edition`      | string | `2024`   | Rust edition for the generated crate     |

### `[rust.dependencies]`

Same syntax as a Cargo `[dependencies]` table. These get copied verbatim into the generated `Cargo.toml`. smelt may *add* dependencies it knows are needed (e.g. `serde_json` if JSON literals appear), but will never override a user-specified version.

### `[strict]`

| Key          | Type | Default | Description                                            |
| ------------ | ---- | ------- | ------------------------------------------------------ |
| `typescript` | bool | `true`  | v1.0: must be `true`. Reserved for relaxation in v2+   |
| `python`     | bool | `true`  | v1.0: must be `true`. Reserved for relaxation in v2+   |

### `[runtime]`

| Key              | Type   | Default        | Description                                         |
| ---------------- | ------ | -------------- | --------------------------------------------------- |
| `clone-strategy` | string | `"aggressive"` | v1.0 only supports `"aggressive"`. Reserved field   |

## Output Layout

Given the example above, `smelt build` produces:

```
dist/
├── Cargo.toml          # generated, do not edit
├── src/
│   ├── main.rs         # from src/main.ts
│   └── ...             # other modules discovered via imports
└── .smelt-cache/       # incremental build state (v2.0+)
```

For `output.kind = "library"`, the crate root is `src/lib.rs` instead of `src/main.rs`.

The generated `Cargo.toml` should never be checked into source control. Add `dist/` to `.gitignore` by default in `smelt new`.

## CLI Commands

| Command                     | Description                                             |
| --------------------------- | ------------------------------------------------------- |
| `smelt new <name>`          | Scaffold a new project with `Smelt.toml`, `src/`, etc.  |
| `smelt build`               | Read manifest, transpile, emit Rust crate               |
| `smelt check`               | Type-check and validate without emitting                |
| `smelt dump-hir <file>`     | Print HIR for a single source file (debug)              |
| `smelt dump-mir <file>`     | Print MIR for a single source file (debug)              |
| `smelt clean`               | Remove `output.target`                                  |

All commands accept `--manifest-path <path>` to override the default search.

## Error Behavior

- Missing `Smelt.toml`: error with suggestion to run `smelt new`.
- Missing required key: error with the section and key name.
- `strict.typescript = false` or `strict.python = false` in v1.0: error explaining strict mode is required.
- Unknown key: warning, not an error. (Future-proofing for newer manifests on older smelt versions.)
