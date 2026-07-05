# smelt-transpiler

The `smelt` command-line transpiler. It takes strictly-typed **TypeScript**
(and, from a source build, **Python**) and lowers it through a shared HIR/MIR
pipeline into idiomatic **Rust**.

```sh
cargo install smelt-transpiler   # installs the `smelt` binary
smelt --help
```

## TypeScript vs. Python

The crate published to crates.io is **TypeScript-only**. The Python frontend
(`smelt-frontend-py`) parses with Astral's Ruff, which is only available as a
git dependency — something crates.io does not allow — so it cannot ship in a
published crate yet (tracked upstream by
[astral-sh/ruff#43](https://github.com/astral-sh/ruff/issues/43)).

To use the Python frontend, build from a source checkout with the `python`
feature (enabled by default in the repository):

```sh
git clone https://github.com/Bombatomica64/smelt
cd smelt
cargo build --release --features python -p smelt-transpiler
# or install the local checkout:
cargo install --path crates/smelt-transpiler --features python
```

A TypeScript-only build reports a clear error if handed a `.py`/`.pyi` file.

## Status

Pre-alpha. See the [project README](https://github.com/Bombatomica64/smelt) and
the milestone issues for the v1.0 roadmap.

## License

[GNU General Public License v3.0](https://github.com/Bombatomica64/smelt/blob/main/LICENSE).
