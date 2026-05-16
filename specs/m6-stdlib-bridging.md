# M6: Standard Library Bridging

**Milestone:** v1.0
**Estimated duration:** 4–6 weeks
**Depends on:** M5

## Goal

Map common TypeScript and (later) Python standard library APIs to their Rust equivalents.

## Why this matters

Without stdlib mapping, even trivial programs fail to compile because `Array.map` has no Rust equivalent without translation. This milestone is what makes the headline demos in M7 and M9 actually work — it's the glue between "your code uses normal idioms" and "the output is normal Rust."

## Scope

### Core mapping module

A `smelt-codegen-rust::stdlib` module that maps known method/function calls to Rust equivalents. The mapping is data-driven where possible, with a fallback to hand-written lowerings for complex cases.

### TypeScript / JavaScript mappings (v1.0 priority)

| TS / JS                            | Rust                                                              |
| ---------------------------------- | ----------------------------------------------------------------- |
| `Array.prototype.map`              | `.into_iter().map(...).collect::<Vec<_>>()`                       |
| `Array.prototype.filter`           | `.into_iter().filter(...).collect::<Vec<_>>()`                    |
| `Array.prototype.reduce`           | `.into_iter().fold(...)`                                          |
| `Array.prototype.forEach`          | `.into_iter().for_each(...)`                                      |
| `Array.prototype.find`             | `.into_iter().find(...)`                                          |
| `Array.prototype.includes`         | `.contains(&...)`                                                 |
| `Array.prototype.length`           | `.len()`                                                          |
| `Array.prototype.push`             | `.push(...)`                                                      |
| `Array.prototype.slice`            | `.iter().cloned().skip(a).take(b - a).collect()`                  |
| `String.prototype.split`           | `.split(...).map(String::from).collect::<Vec<_>>()`               |
| `String.prototype.toLowerCase`     | `.to_lowercase()`                                                 |
| `String.prototype.toUpperCase`     | `.to_uppercase()`                                                 |
| `String.prototype.length`          | `.chars().count()` (note: not `.len()` for Unicode correctness)    |
| `JSON.stringify`                   | `serde_json::to_string(&...)?`                                    |
| `JSON.parse`                       | `serde_json::from_str(...)?`                                      |
| `console.log`                      | `println!("{:?}", ...)`                                           |
| `Object.keys` / `.values` / `.entries` | `.keys()` / `.values()` / `.iter()` on `HashMap`              |
| `Math.floor` / `.ceil` / `.round`  | `.floor()` / `.ceil()` / `.round()`                               |
| `Math.max` / `.min`                | `std::cmp::max` / `std::cmp::min` or `.max()` / `.min()`          |
| `setTimeout` (in async context)    | `tokio::time::sleep`                                              |
| `fetch`                            | `reqwest::get(...).await?` (adds `reqwest` as a dependency)       |

### Runtime helpers

Anything that can't be lowered inline goes into `smelt-runtime` as a helper. Examples:

- JS-style truthy/falsy coercion (try to avoid; reject in the frontend instead).
- `String` ↔ `i64` parsing with JS-like permissive semantics.
- Date/time helpers if `Date` is supported in v1.0 (decide early — deferring is fine).

Keep the runtime crate small. Each helper should justify its existence in a doc comment.

### Auto-dependency injection

When the mapping uses an external crate (e.g. `reqwest`, `serde_json`), the codegen must record this and add it to the generated `Cargo.toml`. If the user pinned a version in `Smelt.toml`'s `[rust.dependencies]`, use that version. Otherwise pick a sensible default and document it.

## Exit Criteria

- [ ] All mappings in the table above implemented and tested.
- [ ] `smelt-runtime` exists with documented helpers.
- [ ] Auto-dependency injection works for `serde_json` and `reqwest` end-to-end.
- [ ] At least 30 golden tests covering stdlib usage.
- [ ] Mapping table is documented in `specs/stdlib-mapping.md`.

## Out of Scope

- Python stdlib mapping (M8/M9 — same module, different mappings).
- Full coverage of TS/JS stdlib. v1.0 covers the subset needed for the Express demo plus high-frequency methods. Anything missing produces a clear "unsupported stdlib call" error.
- DOM APIs. smelt is for server-side code.

## Notes

The TS/JS `String.prototype.length` → `.chars().count()` mapping is intentional and worth calling out: JS strings are UTF-16, Rust strings are UTF-8, and `.len()` returns bytes. This is exactly the kind of subtle correctness issue that makes "transpile, don't translate" the wrong mental model. We are translating; we are choosing semantics.
