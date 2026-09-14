# Hono family H11 — `String` field reads: text and type decided separately

Not on the plan's H1–H10 list. Found while building the H2 runtime tier: the
first draft of a replacer fixture used `` `${match.length}` `` and the generated
crate did not compile. Reducing it showed the shape has nothing to do with
replacers — it fails for a one-parameter callback too — so it is its own family.

## 1. Wrong output

```ts
export const countAll = (input: string): string =>
  input.replace(/[a-z]+/g, (match) => `${match.length}`)
```

Generated Rust (before):

```rust
let _smelt_tmp_1: String = "".to_owned() + &match (closure_arg_0.chars().count() as i64).clone() {
    SmeltUnknown::Null => String::new(),
    SmeltUnknown::Undefined => "undefined".to_owned(),
    …
};
```

`error[E0308]: expected i64, found SmeltUnknown` — six times, once per arm. The
JavaScript `ToString` match for an ERASED value was applied to a **concrete
`i64` expression**.

Note where it does not happen: at module scope, `` `${s.length}` `` emits
`let _smelt_tmp_1: f64 = s.chars().count() as f64;` and then
`_smelt_tmp_1.to_string()`. The frontend recognises `.length` there and lowers a
typed length operation. Inside a closure the parameter's type is not yet known
when the body is lowered (the specializer refines it to `String` later), so the
read stays a generic `Place::Field` and the EMITTER has to type it.

## 2. Responsible functions

Two halves of one decision, in three files, each restating the table:

| what | where | said |
| --- | --- | --- |
| the Rust text | `Emitter::string_field_text`, `emitter/call_runtime.rs:2668` | `.length` -> `({recv}.chars().count() as i64)` |
| the type, for coercion | `Emitter::field_access_type`, `emitter/call_runtime.rs:2073` | `.length` -> `Type::Int` |
| the type, for an operand | `Emitter::place_ty`, `emitter/types.rs:604` | **nothing** — fell through to `Unknown` |
| the type, when erasing | `emitter/coercion.rs:71` | `.length` -> `Type::Int` |

`string_like_operand_text` (`emitter/strings.rs:697`) picks the string coercion
from `self.operand_ty(operand)`, which goes through `place_ty`. `place_ty` had a
`Type::List | Type::Set` arm for `.length` — added earlier for exactly this
reason, with a comment saying so — but no `Type::String` arm. So the text said
`i64` and the type said `Unknown`, and the caller coerced accordingly.

There is a second, smaller fault: `Type::Int` is not necessarily in the
program's type table. The table is fixed before emission and `Emitter::type_id`
is a *lookup*, not an intern, so naming a type the program does not hold is an
`EmitError`. The repro above has no numeric literal anywhere, so `Int` is
absent — meaning the text `as i64` names a type the program cannot carry. Simply
adding the missing `place_ty` arm therefore turned the miscompile into
`type table does not contain literal operand type Int`, swallowed by an
`or_else` in the replacer path and re-emitted as a reference to an undefined
local.

## 3. Design

The two halves must be **one** decision, and the decision has to be expressible
in the program's own type table. `Emitter::string_field_read(receiver_text,
field) -> (String, TypeId)` now returns the text paired with its type, and all
four readers above go through it:

* `string_field_text` returns `.0`,
* `field_access_type`, `place_ty` and the erasing coercion path take `.1`,
* the erasing path also takes `.0`, so it can no longer pair one field's text
  with another field's type.

For `.length` the numeric spelling follows what the program actually interned:
`Int` when present (a character count is an integer, and this keeps every
existing golden), else `Float`, else the runtime-tagged
`SmeltUnknown::Number(… as f64)`. The last case is a real boundary rather than a
convenience: a program that interned no numeric type at all has nothing concrete
to carry the value, and it arises only when the length is immediately
stringified — there is no data flow to lose. `.global`/`.ignoreCase`/`.multiline`
get the same treatment for `Bool`.

## 4. Generality

The rule is "a field read's rendered text and its reported type come from one
place, at a type the program holds". It fires for every `String`-receiver field
read in every position — module scope, closure body, argument, coercion target —
and mentions no library, file, or callback.

## 5. A third defect in the same run

With replacer callbacks now carrying concrete parameter types (family H2),
`needs_unknown_type(mir)` stopped being true for programs whose only regex use
is a callback replacement — and that exposed
`Rvalue::RegexReplaceCallback` missing from `rvalue_needs_regex`
(`crates/smelt-codegen-rust/src/stdlib.rs:87`). The emitted crate used
`regex::Regex::new` and `regex::Captures` without `regex` in its `Cargo.toml`:

```text
error[E0433]: cannot find module or crate `regex` in this scope
```

The variant was simply never listed; it had been pulled in incidentally by the
erasure. Added, with a comment recording why the omission was invisible.

## 6. Tests

`crates/smelt-codegen-rust/tests/regex_replacer_arguments_runtime.rs`,
`a_single_parameter_callback_still_receives_only_the_match` — the `countAll`
fixture above, asserting `'ab cde'` becomes `'2 3'`. It is deliberately placed
in the ONE-parameter test rather than in a multi-argument one, because the
defect is independent of the argument list; the comment in the fixture says so.
That fixture also covers the missing `regex` dependency, since it is a program
whose only regex use is a callback replacement.
