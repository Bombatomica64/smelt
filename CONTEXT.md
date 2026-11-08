# Context — domain vocabulary

Shared nouns for the Smelt compiler. Names here are load-bearing: code, modules,
and architecture reviews should use these exact terms. Add a term when a design
decision names a concept that isn't already here.

## Pipeline

Smelt lowers source through four stages, each behind a small interface:

`frontend (TS/Python) → HIR → MIR → codegen-rust (Rust text)`

- **Type gate** — the checkers (TypeScript 7, ty) run over every source before
  anything lowers, and a lowering's result is held in `Checked<T>` until its
  caller decides what their errors mean (`TypeErrorPolicy`): `smelt build`
  denies by default, `--allow-type-errors` / `[build] type-errors = "allow"`
  opts out (`crates/smelt-transpiler/src/type_gate.rs`).
- **Checker program** — what the checkers see is built from the same resolved
  module graph lowering uses (`ModuleGraph`, `crates/smelt-transpiler/src/check_program.rs`):
  every source Smelt resolved (local files, `[packages]`, typed versions laid
  in memory), each bare specifier mapped to the file Smelt chose, the vendored
  `@types/node`, in-memory stubs for modules of the other language (a `.d.ts`
  of a Python module, a `.pyi` of a TypeScript one), and, in a test file, the
  test API Smelt models as globals. An import the checker cannot resolve but
  Smelt did is `smelt::checker-resolution-drift`, a Smelt bug.
- **Cross-language seam** — TypeScript and Python modules import each other;
  a name is one HIR symbol whichever frontend interned it first
  (`SymbolInterner::intern_shared`), text crossing the seam converts between
  JavaScript's UTF-16 `String` and Python's `PyStr` (`smelt_hir::language_view`),
  and the matrix in `crates/smelt-transpiler/tests/cross_language_matrix_tests.rs`
  checks both directions against Node and CPython.
- **Source trust** — whether the checker accepted a file (`SourceTrust::Checked`)
  or not (`Unchecked`); frontend shortcuts that rely on a checked program refuse
  an unchecked source with `smelt::ill-typed-source` instead of miscompiling it.
- **Specialization** — build-time partial evaluation of definition-time
  metaprogramming by a sandboxed host runtime before frontend lowering.
- **Specialization artifact** — cache-addressed, versioned manifest carrying
  materialized definitions from specialization into pure frontend lowering or
  across a package seam.
- **HIR** — high-level IR produced by a frontend (`to_hir`). Closest to source shape.
- **MIR** — mid-level IR with an explicit CFG and a flat `Rvalue` algebra (`lower_hir`).
- **codegen-rust** — emits Rust source text from MIR (`emit_source`).

## Runtime

- **Runtime item** — a marked region of real Rust in `smelt-runtime`, rendered
  into a generated crate under the program's flags. The crate is the source of
  truth; generated crates inline the text and do not depend on it.
- **Runtime demand** — what a program needs of the runtime: root needs from one
  scan of the MIR, closed over the declared `requires` graph (`runtime_demand.rs`).
  Every need's closure on its own renders a self-contained runtime (checked per
  need), and the emitter writes a family's runtime text only with the proof the
  demand hands out (`RuntimeDemand::require`, `carrier()` for the erased
  carrier), so a use the scan did not predict fails at its emit site.

## SmeltUnknown

The erased runtime value type. It is the ABI for genuinely dynamic boundaries:
source `unknown`, erased interop, JSON/plugin values, and values inspected through
runtime narrowing. It is **not** the default carrier for values that still have a
useful static shape — prefer concrete Rust types, then scoped generics, then
`SmeltUnknown` only at real dynamic boundaries.

## Rendered Rust

- **RustExpr** — a rendered expression: its text, how it binds (`Precedence`) and
  what its producer knows about it. The emitter asks the value; it never reads
  the text. A `RustExpr` with `Precedence::Unstated` is treated as loose. It has
  no `Display` and no `Deref<str>`: a splice states its position —
  `delimited()` (argument, initializer, tail), `tight()` (receiver, callee,
  operand of a unary operator), `as_binary_operand()`, `as_cast_operand()` — or
  goes through `postfix`, so a splice without a precedence decision does not
  compile. A fact that names part of the text carries that part (a string
  constant's literal token, the place a `.clone()` copies).
- **Template temporary** — a name an emitter template binds around code it
  splices (`|smelt_item| {body}`). Always spelled with an emitter prefix, which
  no program local can render to (the name scope moves source names out of
  it), so spliced code never reads a template binder by accident.
- **RenderedValue** — a `RustExpr` with the MIR type it evaluates to.
- **RustType** — a rendered type and what it spells (`shape`, carrier, generics).
- **RustName** — a Rust identifier claimed from a **NameScope** (one Rust
  namespace: the crate's types, the crate's values, one `impl`, one body, the
  module files). Only the allocator constructs one: it escapes the source
  spelling against the reserved set (keywords, prelude and runtime names, `_`,
  the emitter prefixes `smelt_`/`Smelt`/`__smelt`/`SMELT_`, the trait methods
  the emitter calls on user types) and renames it apart from every name already
  claimed in the scope.
- **Name plan** (`RustNames`) — every declared type, function and method's
  RustName, decided once per crate and keyed by identity (`Symbol`, `FuncId`),
  never by rendering. Lives in `crates/smelt-codegen-rust/src/names.rs`.

## Coercion

The one **seam** that converts a value between its static Rust type and the
erased `SmeltUnknown` form, in the codegen-rust emitter. Every typed↔erased
crossing goes through one of its named verbs; callers express intent, never the
per-`Type` mechanics.

- **Verbs (public)** —
  - `value_at_type(operand, target)` / `rendered_at_type(expr, src, target)` —
    coerce to a concrete target type; the general entry, dispatches internally.
  - `erase(operand)` / `erase_rendered(expr, src)` — box a typed value into
    `SmeltUnknown`.
  - `extract(operand, target)` / `extract_rendered(expr, target)` — pull a
    typed value back out of `SmeltUnknown`.
  - `tag_check(operand, kind)` — runtime *narrowing* ("is this tag a String?"),
    a guard, not value coercion.
- **Why verbs, not a single `value_at_type`** — erase is *target-free*: boxing
  into the `SmeltUnknown` runtime value needs no MIR type, so erase (and the
  text-level extract, whose source is `Unknown`) stay first-class verbs.
- **Behind the seam (private)** — the structural-record / function-shape
  adapters and the per-`Type` match arms that decide the concrete Rust text.

Lives in `crates/smelt-codegen-rust/src/emitter/coercion.rs`, with the `erase`, `extract`
and `tag_check` verbs in `emitter/coercion/{erase,extract,tag}.rs`. The private adapters sit
behind the same seam: a record's field-by-field crossings in `coercion/record.rs` and
`coercion/record_adapter.rs`, function shapes in `coercion/function_adapter.rs`.
