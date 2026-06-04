# Context — domain vocabulary

Shared nouns for the Smelt compiler. Names here are load-bearing: code, modules,
and architecture reviews should use these exact terms. Add a term when a design
decision names a concept that isn't already here.

## Pipeline

Smelt lowers source through four stages, each behind a small interface:

`frontend (TS/Python) → HIR → MIR → codegen-rust (Rust text)`

- **HIR** — high-level IR produced by a frontend (`to_hir`). Closest to source shape.
- **MIR** — mid-level IR with an explicit CFG and a flat `Rvalue` algebra (`lower_hir`).
- **codegen-rust** — emits Rust source text from MIR (`emit_source`).

## SmeltUnknown

The erased runtime value type. It is the ABI for genuinely dynamic boundaries:
source `unknown`, erased interop, JSON/plugin values, and values inspected through
runtime narrowing. It is **not** the default carrier for values that still have a
useful static shape — prefer concrete Rust types, then scoped generics, then
`SmeltUnknown` only at real dynamic boundaries.

## Coercion

The one **seam** that converts a value between its static Rust type and the
erased `SmeltUnknown` form, in the codegen-rust emitter. Every typed↔erased
crossing goes through one of its named verbs; callers express intent, never the
per-`Type` mechanics.

- **Verbs (public)** —
  - `value_at_type(operand, target)` / `value_at_type_text(text, src, target)` —
    coerce to a concrete target type; the general entry, dispatches internally.
  - `erase(operand)` / `erase_value_text(text, src)` — box a typed value into
    `SmeltUnknown`.
  - `extract(operand, target)` / `extract_value_text(text, target)` — pull a
    typed value back out of `SmeltUnknown`.
  - `tag_check(operand, kind)` — runtime *narrowing* ("is this tag a String?"),
    a guard, not value coercion.
- **Why verbs, not a single `value_at_type`** — erase is *target-free*: boxing
  into the `SmeltUnknown` runtime value needs no MIR type. Spelling it as
  `value_at_type(op, <Unknown TypeId>)` would require `Type::Unknown` to be
  interned in the program's type table, which is not guaranteed
  (`type_id(Type::Unknown)` fails otherwise). So erase (and the text-level
  extract, whose source is `Unknown`) must stay first-class verbs.
- **Behind the seam (private)** — the structural-record / function-shape
  adapters and the per-`Type` match arms that decide the concrete Rust text.

Lives in `crates/smelt-codegen-rust/src/emitter/coercion.rs`.
