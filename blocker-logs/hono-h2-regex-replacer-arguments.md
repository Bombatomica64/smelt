# Hono family H2 — the ECMA-262 regex replacer argument list

Probe: `smelt --manifest-path third_party/hono/Smelt.toml check --message-format json`
at `honojs/hono@eebdf7be39abf0a872671835ccce0c4f03ea497a`.
4 occurrences, 3 files, one shape with two opposite instances.

## 1. The sites

| file | line | callback | pattern | capture groups |
| --- | ---: | --- | --- | ---: |
| `src/utils/url.ts` | 26 | `(match, index) => …` | `/\{[^}]+\}/g` | 0 |
| `src/router/reg-exp-router/router.ts` | 26 | `(match, metaChar) => …` | `/([.\\+*[^\]$()])/g` | 1 |
| `src/router/reg-exp-router/prepared-router.ts` | 162 | `(_, str) => …` | `/"##(.+?)##"/g` | 1 |
| `src/middleware/secure-headers/secure-headers.ts` | 266 | `(match, offset) => …` | `/[A-Z]/g` | 0 |

(The `url.ts` site is counted twice by the probe — the same call is reached
through two lowering passes.)

These four are the whole story of the family, because the first two have the
**same callback shape and opposite meanings**:

```ts
'{a}/b'.replace(/\{[^}]+\}/g,      (m, x) => `${x}`)   // x is the POSITION, 0
'a.b'.replace(/([.\\+*])/g,        (m, x) => `\\${x}`)  // x is CAPTURE GROUP 1, '.'
```

## 2. Wrong output

No output: lowering rejects the file with

```text
regex replacement callback must accept a match string and return a string
(Some(Function(FunctionType { params: [TypeId(0), TypeId(15)], … })))
```

Every multi-parameter replacer in existence was rejected. Reduced repro (two
lines) reproduces it exactly.

## 3. Responsible functions

* Frontend: `ModuleBuilder::regex_replace_call`,
  `crates/smelt-frontend-ts/src/lowering/stdlib/numbers_math.rs:16`. It built a
  **fixed one-parameter** contextual type for the callback

  ```rust
  let callback_ty = … Type::Function(FunctionType { params: vec![string_ty], return_ty: string_ty, … });
  ```

  and then rejected anything whose lowered arity was not exactly 1:

  ```rust
  matches!(ty, Type::Function(function) if function.params.len() == 1 && …)
  ```

* Emitter: `Emitter::regex_replace_callback_text`,
  `crates/smelt-codegen-rust/src/emitter/strings.rs:355`. It emitted a
  `Replacer` closure that passed exactly one argument, hard-coded:

  ```rust
  let call_expr = format!("({callback_text})(caps.get(0).expect(\"regex match missing\").as_str().to_string())");
  ```

So the one-argument assumption was duplicated in the two layers, and neither
layer had any representation of the rest of the spec list.

The named-function replacement path 60 lines further down had a related latent
defect: it accepted a function with `required_params <= 1` but any number of
DECLARED parameters, while the emitter still passed one argument. Rust has no
optional parameters, so a two-parameter replacer with one required parameter
would have emitted a call of the wrong arity.

## 4. Design

ECMA-262 `RegExp.prototype[@@replace]` calls the replacer with a fixed
positional list

```text
(matched, p1, …, pN, position, string)
```

where `N` is the **pattern's** capture-group count, and a callback declares a
prefix of it. Three consequences drove the design:

1. **The role of a parameter is a property of the pattern.** So `N` must be
   known where the call is lowered. It is: `regex_replacement_pattern` already
   folds every statically known pattern spelling — a regex literal,
   `new RegExp('…')`, `RegExp('…')`, and an identifier bound to one — to a
   `Literal::String`, so the pattern text can be read back off the lowered
   expression.

2. **Counting capture groups is regex-grammar knowledge**, the same knowledge
   `smelt_stdlib::js_regex::to_rust_pattern` already owns, so
   `capture_group_count` was added beside it rather than as a private helper in
   the frontend. A `(` opens a capture group only when it is outside a character
   class, not escaped, and not one of the non-capturing prefixes — `(?:`, `(?=`,
   `(?!`, `(?<=`, `(?<!`, inline flags — while `(?<name>` *does* capture. A
   `str::matches('(').count()` would have been wrong for six of the eight
   fixtures in its unit test, including two of the four Hono patterns.

3. **The resolved roles belong in the IR, not re-derived in the emitter.** A new
   `smelt_hir::RegexReplaceArg` (`Matched | Capture(u32) | Position | Source`)
   is carried on `ExprKind::RegexReplaceCallback` and
   `Rvalue::RegexReplaceCallback`, so the frontend decides once and the emitter
   only renders. It also shows up in the HIR and MIR goldens
   (`regex_replace_all_callback "…", %0, %1 [matched,p1]`), which makes a
   mis-resolution visible in a cheap test rather than only in generated Rust.

Each role has exactly one type the spec fixes, so the callback's parameters get
**concrete** types instead of erasure:

| role | contextual type | rendered from `caps: &regex::Captures` |
| --- | --- | --- |
| `Matched` | `string` | `caps.get(0)…as_str().to_string()` |
| `Capture(n)` | `string \| undefined` | `caps.get(n).map(…)` |
| `Position` | `number` | characters of the subject before the match |
| `Source` | `string` | the subject |

`Capture(n)` is `Optional(String)` and not `String` because a group inside an
alternation that did not participate is passed `undefined`, not `''` — the
`an_unmatched_capture_group_arrives_as_undefined` fixture would silently change
result if the two were collapsed. `Position` counts CHARACTERS, matching the
index convention the other string helpers in `strings.rs` already use (JS counts
UTF-16 code units; the two agree outside astral planes).

The emitter now binds the subject once before the call —
`{ let smelt_subject: String = <haystack>; regex.replace_all(&smelt_subject, |caps| …) }`
— because `Position` and `Source` read the subject from INSIDE the closure and
re-rendering the operand there would evaluate it twice. Each rendered argument
is routed through `value_at_type_text` to the callback's own declared parameter
type, so a callback that annotated a parameter differently still type-checks.

### What is deliberately still rejected

A callback with **more than one parameter over a pattern whose text is not
statically known** cannot have its roles resolved at all, and is reported:

```text
regex replacement callback declares 2 parameters, but the pattern text is not
statically known, so it cannot be decided which are capture groups and which
are the match position and subject string
```

Guessing "capture group" would hand a `number`-typed parameter a string (or the
reverse) with no diagnostic. A callback declaring more parameters than the spec
supplies is likewise reported rather than truncated. No Hono, es-toolkit, remeda
or radash site hits either case.

## 5. Generality

The rule is stated entirely in terms of the ECMA-262 replacer signature and the
pattern's capture-group count. It fires for any `.replace`/`.replaceAll` with a
function replacement, from any source. Nothing keys off a file, a library, a
callback parameter name, or a pattern spelling.

## 6. Tests

* `crates/smelt-stdlib/src/js_regex.rs` —
  `capture_groups_are_counted_and_group_modifiers_are_not` and
  `literal_parentheses_are_not_capture_groups`: eight patterns covering zero
  groups, lookahead/lookbehind, non-capturing groups, named groups, nesting,
  escaped parens, parens inside a character class, and the two member-less
  classes.
* `crates/smelt-codegen-rust/tests/regex_replacer_arguments_runtime.rs` (new
  tier, registered in the `host` shard of
  `.github/workflows/runtime-tiers.yml`) — five fixtures that COMPILE AND RUN:
  the position case, the capture case (same arity, opposite meaning), an
  unmatched group arriving as `undefined`, the full four-argument list in spec
  order, and a one-parameter callback as a guard that the wider signature did
  not start passing it extra arguments.

## 7. A separate defect this surfaced (see `hono-h11-string-field-read.md`)

The first draft of the runtime fixtures used `` `${match.length}` `` in a
replacer body and did not compile — but the same expression fails in a
ONE-parameter callback too, so it is not part of this family. It is a
text/type divergence on `String` field reads in the emitter, written up
separately and fixed in the same commit.

## 8. Result

4 occurrences -> 0. `prepared-router.ts` leaves the blocker list entirely;
`router.ts` drops from 3 blockers to 1; `url.ts` from 5 to 3.
