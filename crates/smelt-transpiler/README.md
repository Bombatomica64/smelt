# smelt-transpiler

The `smelt` command-line transpiler. It takes strictly-typed **TypeScript** and
**Python** and lowers them through a shared HIR/MIR pipeline into idiomatic
**Rust**.

```sh
cargo install smelt-transpiler   # installs the `smelt` binary
smelt --help
```

## TypeScript and Python

The crate published to crates.io includes both frontends and enables ty-backed
Python type resolution by default.

A TypeScript-only build reports a clear error if handed a `.py`/`.pyi` file.

## Status

Pre-alpha. See the [project README](https://github.com/Bombatomica64/smelt) and
the milestone issues for the v1.0 roadmap.

## License

[GNU General Public License v3.0](https://github.com/Bombatomica64/smelt/blob/main/LICENSE).
