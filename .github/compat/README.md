# Compatibility probe fixtures

Each subdirectory holds a `Smelt.toml` that points Smelt at a third-party
TypeScript or Python library's source root + test glob. They are used to probe
how far Smelt can transpile real-world "bug libraries".

`remeda/` is the curated, passing compat target wired into the
`Compatibility` GitHub Actions workflow. The others are exploratory probes run
daily by the [`Library Probes`](../workflows/library-probes.yml) workflow, which
refreshes the report at
[`blocker-logs/library-probes.md`](../../blocker-logs/library-probes.md).

The probe set, pinned refs, and per-library source roots live in
[`libraries.json`](./libraries.json); the driver is
[`scripts/probe_libraries.py`](../../scripts/probe_libraries.py). Bump a `ref`
deliberately to re-baseline a probe against a newer library version.

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

## Native single-library probe

`smelt probe` reports the same information for one manifest without the external
Python scripts — it consumes frontend diagnostic categories directly:

```bash
# Markdown blocker report (transpile verdict + blockers grouped by category)
cargo run --bin smelt -- --manifest-path lib/Smelt.toml probe

# Machine-readable JSON, and also run the generated tests when it transpiles
cargo run --bin smelt -- --manifest-path lib/Smelt.toml probe --format json --run-tests
```

## Reproducing the whole report locally

```bash
cargo build --bin smelt
mkdir -p target/library-probes
# clone each library at its pinned ref into target/library-probes/<name>, then:
python3 scripts/probe_libraries.py \
  --config .github/compat/libraries.json \
  --fixtures .github/compat \
  --work-dir target/library-probes \
  --output blocker-logs/library-probes.md
```
