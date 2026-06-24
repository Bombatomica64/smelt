# Compatibility probe fixtures

Each subdirectory holds a `Smelt.toml` that points Smelt at a third-party
TypeScript or Python library's source root + test glob. They are used to probe
how far Smelt can transpile real-world "bug libraries".

`remeda/` is the curated, passing compat target wired into the
`Compatibility` GitHub Actions workflow. The others are exploratory probes;
the most recent results are written up in
[`blocker-logs/bug-library-probes-2026-06-24.md`](../../blocker-logs/bug-library-probes-2026-06-24.md).

## Probed libraries

| Dir | Source repo | Language |
| --- | --- | --- |
| `remeda` | `remeda/remeda` | TS (reference, passes) |
| `es-toolkit` | `toss/es-toolkit` | TS |
| `radash` | `sodiray/radash` | TS |
| `ts-pattern` | `gvergnaud/ts-pattern` | TS |
| `valibot` | `fabian-hiller/valibot` | TS |
| `neverthrow` | `supermacro/neverthrow` | TS |
| `returns` | `dry-python/returns` | Py |
| `result` | `rustedpy/result` | Py |
| `more-itertools` | `more-itertools/more-itertools` | Py |
| `funcy` | `Suor/funcy` | Py |
| `toolz` | `pytoolz/toolz` | Py |

## Reproducing a probe

```bash
# 1. Fetch the library source (default branch) next to Smelt.
curl -L -o lib.tar.gz https://codeload.github.com/<org>/<repo>/tar.gz/refs/heads/main
mkdir lib && tar -xzf lib.tar.gz -C lib --strip-components=1

# 2. Drop the fixture manifest into the library root.
cp .github/compat/<name>/Smelt.toml lib/Smelt.toml

# 3. Try a whole-crate transpile.
cargo run --bin smelt -- build --manifest-path lib/Smelt.toml

# 4a. If it transpiles, run the generated tests.
cargo run --bin smelt -- rust-test-report \
  --build-manifest lib/Smelt.toml \
  --cargo-manifest lib/dist-smelt/Cargo.toml --full --output report.md

# 4b. If the build aborts at the first unsupported file, enumerate every
#     blocker class across the whole library:
SMELT_ROOT=$PWD python3 scripts/probe_blocker_scan.py lib ts src
```
