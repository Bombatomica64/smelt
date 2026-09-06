# es-toolkit final-45 investigation — group: JSON / STRINGS / PANICS / TEST MATCHERS

Read-only investigation. Generated crate inspected at
`/home/user/smelt/third_party/es-toolkit/dist-smelt/src/`; no cargo was run. One
empirical experiment was run with `rustc` directly against the prebuilt
`fancy_regex` rlib (see §4) — no cargo invocation.

Six tests, five independent root families.

---

## 1. `isJSON_spec::test_isjson_returns_false_if_the_value_is_not_a_valid_json_string`

**Spec** `src/predicate/isJSON.spec.ts:15-21`; first failing assertion
`expect(isJSON('invalid json')).toBe(false)` (line 16).
**Source** `src/predicate/isJSON.ts:33-38`:

```ts
  try {
    JSON.parse(value);
    return true;
  } catch {
    return false;
  }
```

JS answer: `JSON.parse` throws `SyntaxError`, the `catch` returns `false`.

**Generated Rust** (`dist-smelt/src/isJSON.rs:7-19`, whole function):

```rust
pub(crate) fn is_json(value: SmeltUnknown) -> bool {
    let _smelt_tmp_1: bool = matches!(value.clone(), SmeltUnknown::String(_));
    let _smelt_tmp_2: bool = !(_smelt_tmp_1);
    if _smelt_tmp_2 {
    return false;
    } else {
    _smelt_tmp_3 = match value.clone() { /* String(value) => value.to_string(), … */ };
    _smelt_tmp_4 = match (serde_json::from_str::<SmeltUnknown>(&_smelt_tmp_3).expect("JSON parse failed")).into_smelt_unknown() { … };
    return true;
    }
}
```

Two things are wrong and they are one root:

1. `JSON.parse` is emitted as an **infallible panicking expression**
   (`.expect("JSON parse failed")`) — this is the panic in the backtrace
   (`src/isJSON.rs:16:79`).
2. **The whole `catch { return false }` arm is gone.** Because the emitted try
   body contains no fallible operation, MIR gave the try region no throwing edge,
   so the catch block has no predecessor and is dropped. `is_json` does not even
   return `Result`. A JS-catchable error became an abort.

**Root cause / layers**

* Emitter: `crates/smelt-codegen-rust/src/emitter/map.rs:990-1022`,
  `Emitter::json_parse_text` — both arms format
  `serde_json::from_str::<…>(&…).expect("JSON parse failed")`.
* MIR lowering: `crates/smelt-mir/src/lower/expr.rs:1867-1870`,
  `ExprKind::JsonParse` → `self.assign_temp(…, Rvalue::JsonParse { … })`, a plain
  `Statement::Assign`. Only `Terminator::Call` and `Terminator::Await` carry
  `unwind: Option<ExceptionHandler>` (`crates/smelt-mir/src/types.rs:1906-1934`),
  so a throwing *rvalue* cannot reach a handler at all. Contrast the `Await` arm
  20 lines below at `expr.rs:2225-2240`, which does exactly the right thing:

  ```rust
  if self.current_exception_handler().is_some() {
      let target = self.function.push_block(expr.span);
      self.set_terminator(Terminator::Await { future, dest, target,
          unwind: self.current_exception_handler() })?;
  ```
* Frontend: `crates/smelt-frontend-ts/src/lowering/stdlib.rs:1264` / `:1308` push
  `ExprKind::JsonParse` and nothing marks the enclosing function `may_throw`.

**Shares a root with**: nothing else in the 45 (it is the only `JSON parse
failed` panic), but the *mechanism* — a fallible stdlib rvalue that cannot reach
an active `try` — is a general hole that any future throwing rvalue hits.

**Verdict: (a) general defect, fixable. Size M.**

Fix design (mirrors machinery that already exists, no new emitter shapes):

1. MIR: add `BuiltinFn::JsonParse` to `crates/smelt-mir/src/types.rs:2027-2035`
   and lower `ExprKind::JsonParse` through the terminator form —
   `Terminator::Call { callee: Callee::Builtin(BuiltinFn::JsonParse), args:
   [text], dest, target, unwind: self.current_exception_handler() }` — keeping the
   statement/`Rvalue::JsonParse` form only when no handler is active *and* the
   function is not `may_throw` (the exact structure of the `Await` arm).
2. Runtime prelude: a `smelt_json_parse(&str) -> Result<SmeltUnknown, Box<dyn
   std::error::Error>>` helper whose `Err` is the same thrown-error record shape
   `new Error(...)` already produces (see `random_1.rs:23` for the emitted
   payload). Emitter renders the builtin as `smelt_json_parse(&text)?`.
3. Nothing else is needed for the catch arm: the emitter already has
   `emit_throwing_call_terminator`
   (`crates/smelt-codegen-rust/src/emitter/control_flow.rs:1116-1215`), whose
   `Ok(Ok(v))/Ok(Err(e))/Err(panic)` shape binds the exception local and jumps to
   `handler.catch_block`. Its non-`?` branch (`catch_unwind`) is even a safe
   interim: routing JSON.parse through the terminator with the *current*
   panicking text would already make `isJSON` return `false`.
4. Propagate `may_throw` to the enclosing HIR function when a body contains a
   throwing stdlib op, so an uncaught `JSON.parse` becomes a thrown error rather
   than a panic.

Regression test shape: a `smelt-codegen-rust` inline test transpiling
`export function f(s: string): boolean { try { JSON.parse(s); return true } catch { return false } }`
and asserting the emitted source (i) contains no `expect("JSON parse failed")`,
(ii) contains a match on the parse result with an arm that returns `false`.
Plus the es-toolkit `isJSON` suite.

Side note (not the failure): `_smelt_tmp_4` is typed
`SmeltRecord<String, SmeltUnknown>` and immediately discarded — the parse result
of a value-less `JSON.parse(value);` statement is given a Dict destination and a
full record-coercion match. Harmless, but dead code worth dropping.

---

## 2. `isJSONValue_spec::test_isjsonobject_isjsonobject_should_return_false_for_not_valid_value`

**Spec** `src/predicate/isJSONValue.spec.ts:74`:
`expect(isJSONObject({ undefinedProperty: undefined })).toBe(false)`.
JS answer: `Reflect.ownKeys` yields `['undefinedProperty']`, `obj[key]` is
`undefined`, `isJSONValue(undefined)` hits `typeof === 'undefined'` → `default:
return false`. So `false`.

**Generated Rust** (`dist-smelt/src/isJSONValue_spec.rs:389-390`):

```rust
_smelt_tmp_26 = SmeltRecord::from([("undefinedProperty".to_owned(), ())]);
let _smelt_tmp_27: bool = is_json_object({ let smelt_record = _smelt_tmp_26.clone();
    SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id,
        smelt_record.iter().map(|(key, value)| (key, SmeltUnknown::Null)).collect())) });
```

`undefined` became **`SmeltUnknown::Null`**. The map closure does not even read
`value` — the per-entry erasure is a *constant* because the record's value type is
the unit type `()`. `is_json_value(Null)` takes the `"object"` arm
(`isJSONValue.rs:10-15`: `Null | Array | Object | Promise => "object"`) and
`matches!(value, SmeltUnknown::Null)` returns `true`, so `is_json_object` returns
`true`. JS says `false`.

**Root cause / layer: codegen-rust emitter.**
`crates/smelt-codegen-rust/src/emitter/coercion.rs:1168-1178` (the
`Some(Type::Dict(key, item))` arm of `erase`) builds
`value_wrap = self.erase_value_text("value", *item)`, and for `item ==
Type::None` the type-driven answer is the constant `"SmeltUnknown::Null"`
(`coercion.rs:1106` in `erase`, twin at `coercion.rs:1514` in `erase_value`).
HIR/MIR are not at fault by design: per `specs/distinct-undefined.md` the
`null`/`undefined` distinction is deliberately carried as
`Constant::Undefined` on the *operand*, not as a type — and
`erase(Operand::Const(Constant::Undefined))` at `coercion.rs:1056-1059` does
return `SmeltUnknown::Undefined`. The dict path never consults the operand, so
the distinction is dropped. The list path already has the recovery this arm is
missing: `list_local_all_undefined_constants`
(`coercion.rs:1367-1414`) walks the defining `Rvalue::List` and checks every
element for `Constant::Undefined`.

**Shares a root with**: very likely `zip_spec::test_zip_zips_multiple_arrays_to_create_a_tuple`
(its expected value declares `_smelt_tmp_39: SmeltList<()>` for `[3, undefined]`)
and plausibly the `undefined`-returning tail (`at`, `maxBy`, `minBy`,
`reduceAsync`, `mergeWith`) — same `Type::None` → `Null` erasure family. Not
verified here; those are other groups.

**Verdict: (a) general defect, fixable. Size S–M.**

Fix design: add the dict twin of the existing list recovery in
`emitter/coercion.rs`. When erasing an operand of type `Dict(String, None)`
(equally `List(None)`, `Tuple` containing `None`), resolve the defining
`Rvalue::Dict(Vec<(Operand, Operand)>)`
(`crates/smelt-mir/src/types.rs:610`) for that local and erase **each entry's
value operand through `self.erase(operand)`** — emitting an explicit entry list
`SmeltObject::with_id(id, vec![(k, SmeltUnknown::Undefined), …])` instead of the
type-driven `.map(|(key, value)| (key, <const>))` closure. Fall back to the
current Null constant when no single defining `Rvalue::Dict` is available (same
conservatism as `list_local_all_undefined_constants`). Better still, factor the
"recover `Constant::Undefined` from the defining rvalue" step into one helper
used by the List, Tuple and Dict arms, so the next container type does not repeat
the bug.

Regression test shape: codegen inline test on
`const o = { a: undefined }; const u: unknown = o;` asserting the emitted
erasure contains `SmeltUnknown::Undefined` and not `SmeltUnknown::Null`; plus a
runtime-behaviour test that `typeof (o as any).a === 'undefined'` after a round
trip through `unknown`.

---

## 3. `isJSONValue_spec::test_isjsonobject_isjsonobject_should_return_false_when_key_is_not_a_string`

**Spec** `src/predicate/isJSONValue.spec.ts:80`:
`expect(isJSONObject({ [Symbol('a')]: 'a' })).toBe(false)`.
JS answer: `Reflect.ownKeys` returns `[Symbol(a)]`, `typeof key !== 'string'` →
`false`.

**Generated Rust.** The object literal keeps the symbol key correctly
(`isJSONValue_spec.rs:399-401`): a `SmeltJsMap<SmeltUnknown, String>` erased with
`smelt_property_key(key)`, which stores it as the prefixed string key
`__smelt_symbol:Symbol(a)@3103` (prelude emitted at
`crates/smelt-codegen-rust/src/lib.rs:3071`).

The defect is in the callee, `dist-smelt/src/isJSONValue.rs:75-99`:

```rust
    let keys: SmeltList<String>;
…
    _smelt_tmp_8 = Into::<SmeltList<_>>::into(smelt_host_buffer_record_index_keys(&_smelt_tmp_7)
        .unwrap_or_else(|| _smelt_tmp_7.keys()
            .filter(|key| !key.starts_with("__smelt_symbol:") && smelt_is_for_in_record_key(&_smelt_tmp_7, key))
            .collect::<Vec<_>>()));
…
    if false {
    return false;
    } else {
```

Two consequences, one root:

1. `Reflect.ownKeys(obj)` was lowered as **`Object.keys`** — the enumeration
   explicitly **filters out `__smelt_symbol:` keys**. For this object `keys` is
   empty, the loop body never runs, and `is_json_object` returns `true`.
2. Because `keys` is typed `SmeltList<String>`, the source test
   `typeof key !== 'string'` const-folds to the literal **`if false`**. That fold
   is *correct given the type*; the type is what is wrong.

**Root cause / layer: frontend-ts.**
`crates/smelt-frontend-ts/src/lowering/stdlib/objects.rs:243`, in
`object_projection_call`:

```rust
            ("Reflect", "ownKeys") => DictProjectionOp::Keys,
```

with the doc comment above it (`objects.rs:222-227`) claiming "the two return the
same string-key list, since Smelt records carry no non-enumerable or symbol
keys". That premise is false today: symbol-keyed properties *are* stored (as
`__smelt_symbol:<desc>`) and there is already a projection that reads them,
`DictProjectionOp::Symbols` (`crates/smelt-hir/src/expr/ops.rs:280-281`, emitted
at `crates/smelt-codegen-rust/src/emitter/map.rs:873`).

**Shares a root with**: no other test in the 45 (grep shows this is the only
`Reflect.ownKeys`-on-symbol-keys case), but it also silently affects any
`Reflect.ownKeys` consumer that expects symbol keys.

**Verdict: (a) general defect, fixable. Size M.**

Fix design:

* Add `DictProjectionOp::OwnKeys` (`crates/smelt-hir/src/expr/ops.rs:273-286`)
  and map `("Reflect", "ownKeys")` to it in `object_projection_call`. Its HIR
  result type is `List(Unknown)` — the same choice the `Symbols` arm already
  documents at `objects.rs:308-320`, because a key here is genuinely
  `string | symbol`, i.e. a real dynamic boundary (this is not new
  `SmeltUnknown` for a typed shape: the source type of `Reflect.ownKeys` *is*
  `(string | symbol)[]`).
* Emitter (`emitter/map.rs`, the `DictProjection` arm around lines 815-873):
  render `OwnKeys` as the concatenation of the existing string-key projection,
  mapped to `SmeltUnknown::String`, and the existing symbol projection
  (`strip_prefix("__smelt_symbol:") → SmeltUnknown::Symbol`), string keys first
  (JS own-key order: integer keys, then string keys in insertion order, then
  symbols).
* With `key: SmeltUnknown`, `typeof key !== 'string'` stops folding and becomes
  the runtime tag test, and `obj[key]` already routes a `SmeltUnknown::Symbol`
  index back through `smelt_property_key`, so the value read keeps working.

Regression test shape: a frontend/codegen test that
`Reflect.ownKeys({ [Symbol('a')]: 1, b: 2 })` produces a `List<Unknown>` of two
elements with the symbol preserved, and asserting the emitted source for
`typeof k === 'string'` over such a key is *not* const-folded; plus the
es-toolkit `isJSONObject` suite.

---

## 4. `escapeRegExp_spec::test_escaperegexp_should_escape_values`

**Spec** `src/string/escapeRegExp.spec.ts:9`:
`expect(escapeRegExp(unescaped + unescaped)).toBe(escaped + escaped)` with
`unescaped = '^$.*+?()[]{}|\\'`.
**Source** `src/string/escapeRegExp.ts:13`:
`return str.replace(/[\\^$.*+?()[\]{}|]/g, '\\$&');`

**Generated Rust** (`dist-smelt/src/escapeRegExp_1.rs:7-10`, whole function):

```rust
pub(crate) fn escape_reg_exp_794(str: String) -> String {
    let _smelt_tmp_1: String = SmeltRegExp::new("[\\\\^$.*+?()[\\]{}|]".to_owned(), String::new())
        .replace_string(&str.clone(), &"\\$&".to_owned(), true);
    return _smelt_tmp_1;
}
```

**Two independent defects, both verified empirically** (compiled a 20-line
`rustc` program against the crate's own prebuilt
`libfancy_regex-703a68d623cd76db.rlib`, no cargo):

1. **The pattern does not compile.** `fancy_regex::Regex::new("[\\^$.*+?()[\]{}|]")`
   →`Parsing error at position 18: Invalid character class`. In JS an unescaped
   `[` inside a character class is a literal `[`; in `regex-syntax` it opens a
   *nested* class, so the outer class is left unterminated. The prelude swallows
   this: `try_compiled()` is `fancy_regex::Regex::new(&pattern).ok()`
   (`crates/smelt-codegen-rust/src/lib.rs:4416`) and `replace_string` begins
   `let Some(regex) = self.try_compiled() else { return haystack.to_owned(); };`
   (`lib.rs:4439`) — a JS-valid regex silently degrades to a **no-op**, which is
   also why the sibling test `escapeRegExp('abc')` "passes".
2. **JS replacement patterns are not expanded.** With the class hand-fixed to
   `[\\^$.*+?()\[\]{}|]` the same program returns
   `"\\$&\\$&\\$&…"` — because `replace_string` pushes the replacement verbatim
   (`crates/smelt-codegen-rust/src/lib.rs:4444-4446`):

   ```rust
   for matched in regex.find_iter(haystack).filter_map(Result::ok) {
       output.push_str(&haystack[last_end..matched.start()]);
       output.push_str(replacement);
       last_end = matched.end();
   }
   ```

   None of `$$`, `$&`, `` $` ``, `$'`, `$n`, `$<name>` is honoured anywhere —
   `grep -rn '\$&'` over the transpiler crates finds no handling at all.

Both must be fixed for this test to pass.

**Root cause / layers**

* Regex source translation: **frontend-ts**,
  `crates/smelt-frontend-ts/src/lowering/decls/arrows.rs:833-838`,
  `Lowering::rust_regex_pattern_text` — currently four literal `str::replace`
  hacks:

  ```rust
      pattern.replace("(?<", "(?P<")
             .replace("\\.{0,4096}", "\\.*")
             .replace(".{0,4096}?", ".*?")
             .replace("[^.[\\]]", "[^.\\[\\]]")
  ```

  The last one is a verbatim special case for a single library's spelling — the
  exact thing CLAUDE.md "Type lowering" forbids — and it is why some other
  patterns survive and this one does not.
* Replacement expansion: **runtime prelude**, emitted by
  `crates/smelt-codegen-rust/src/lib.rs:4437-4457`
  (`SmeltRegExp::replace_string`).

**Shares a root with**: `estk-remaining-triage.md:200` already recorded
"`escapeRegExp` needs JS→Rust character-class translation" — confirmed, and the
`$&` half is new. The silent `try_compiled → None → no-op` also hides any other
untranslated pattern in the corpus.

**Verdict: (a) general defect, fixable. Size M.**

Fix design:

1. Replace `rust_regex_pattern_text` with a small **character-class-aware
   rewriter** over the JS pattern (single left-to-right scan tracking
   in-class / escaped state), dropping all four literal replacements:
   inside a class, escape a bare `[` (and a bare `^` that is not leading, and
   `&&`, which `regex-syntax` reads as class intersection); outside a class,
   translate `(?<name>` → `(?P<name>`; keep `[^]` → `(?s:.)`. Every rule is
   stated as a JS-vs-Rust syntax difference, not as a pattern-text match.
2. Make an untranslatable pattern **loud**: either a transpile-time diagnostic
   or a thrown `SyntaxError` at construction, never the current silent
   `haystack.to_owned()` no-op.
3. Implement JS replacement-pattern expansion in `replace_string` (and the
   single-match branch below it): expand `$$`, `$&`, `` $` ``, `$'`, `$n`/`$nn`,
   `$<name>` from the `fancy_regex::Captures` of each match, leaving any other
   `$x` literal, per the ECMA-262 `GetSubstitution` table. This needs
   `captures_iter`, not `find_iter`.

Regression test shape: a runtime/prelude unit test table over
`'a-b'.replace(/-/g, '$&$&')`, `'ab'.replace(/(a)(b)/, '$2$1')`,
`'x'.replace(/x/, '$$')`, `/[\]\[]/` and `/[a[b]/` compiling and matching the
literal `[`; plus the es-toolkit `escapeRegExp` suite.

---

## 5. `randomInt_spec::test_randomint_generates_a_random_integer_between_0_inclusive_and_max_exclusive`

**Spec** `src/math/randomInt.spec.ts:16-23`, failing at `randomInt(5)` (line 18).
**Source** `src/math/randomInt.ts:43-45` (the implementation signature of a
3-overload declaration):

```ts
export function randomInt(minimum: number, maximum?: number): number {
  return Math.floor(random(minimum, maximum!));
}
```

JS answer: `maximum!` is **type-level only**; `undefined` is passed to `random`,
whose implementation signature is `random(minimum: number, maximum?: number)` and
which begins `if (maximum == null) { maximum = minimum; minimum = 0; }`. So
`randomInt(5)` is a random integer in `[0, 5)`.

**Generated Rust** (`dist-smelt/src/randomInt.rs:7-13`, whole function):

```rust
pub(crate) fn random_int(minimum: f64, maximum: Option<f64>) -> Result<f64, Box<dyn std::error::Error>> {
    let _smelt_tmp_2: f64 = maximum.clone().clone().expect("optional value was absent after narrowing");
    let _smelt_tmp_3: f64 = random_132(minimum, Some(_smelt_tmp_2))?;
    _smelt_tmp_4 = _smelt_tmp_3.floor();
    return Ok(_smelt_tmp_4);
}
```

The call site is `random_int(5.0, None::<f64>)` (`randomInt_spec.rs:80`), so the
`.expect` panics — the backtrace's `src/randomInt.rs:9:53`. Note the emitted
sequence is self-cancelling: **`Some(x.expect(…))` where `x: Option<f64>` is just
`x`**, and the callee's parameter is `Option<f64>` and handles `None`
correctly (`random_1.rs:16-19`).

**Root cause / layers**

* HIR lowering: `crates/smelt-frontend-ts/src/lowering/expr/operators.rs:2277-2294`,
  `non_null_assertion_value` — a `!` pushes
  `ExprKind::TypeAssert { value }` **retyped to the non-nullish type**, i.e. the
  `!` is modeled as a type change from `Optional(f64)` to `f64`.
* Emitter: `crates/smelt-codegen-rust/src/emitter/call_runtime.rs:222-236` — the
  `Rvalue::Use(operand)` arm whose operand type is `Optional(inner)` and whose
  destination is `inner` renders
  `"{}.clone().expect(\"optional value was absent after narrowing\")"`.

That emitter rule is *right* for flow narrowing — inside `random_132`
(`random_1.rs:20`) the same text is provably safe after
`if maximum.is_none() { maximum = Some(minimum) }`. It is wrong for a source-level
`!`, which asserts nothing at runtime and here loses the `None` on the way into
an `Option`-typed parameter.

**Shares a root with**: the only `optional value was absent after narrowing`
panic in the 45.

**Verdict: (a) general defect, fixable. Size S.**

Fix design (two complementary rules; the first alone fixes this test):

1. **Frontend, argument lowering.** In
   `crates/smelt-frontend-ts/src/lowering/guards.rs:1340-1347`, the
   `Argument::TSNonNullExpression` arm already lowers the inner expression *with
   the parameter's type hint*:

   ```rust
   Argument::TSNonNullExpression(non_null) => {
       let value = self.expression_with_hint(&non_null.expression, body, type_hint)?;
       Ok(self.non_null_assertion_value(value, …, body))
   }
   ```

   Skip `non_null_assertion_value` when the hint is itself
   `Type::Optional(_)` (more precisely: when the hint's `non_nullish_type`
   differs from the hint) — the sink accepts the nullish value, so `!` has
   nothing to narrow and must stay a no-op, exactly as in JS.
2. **Emitter/MIR peephole.** Cancel `Some(x.expect("optional value was absent
   after narrowing"))` back to `x` wherever an `Optional → inner` narrowing use
   is immediately re-wrapped for an `Optional` destination (either in
   `emitter/call_runtime.rs` next to the rule above, or as a MIR
   `opt` pass). This kills the same defect for the paths that do not carry an
   argument hint, and also removes the redundant `.clone().clone()`.

Regression test shape: a frontend/codegen test on
```ts
function inner(a: number, b?: number): number { return b == null ? a : b; }
export function outer(a: number, b?: number): number { return inner(a, b!); }
```
asserting the emitted `outer` contains neither `expect("optional value was
absent after narrowing")` nor `Some(`, and forwards `b` directly; plus the
es-toolkit `randomInt` suite.

---

## 6. `sampleSize_spec::test_samplesize_returns_a_sample_element_array_of_a_specified_size`

**Spec** `src/array/sampleSize.spec.ts:9`:
`expect(array).toEqual(expect.arrayContaining(result));`
JS answer: `expect.arrayContaining(result)` is a vitest **asymmetric matcher**;
`toEqual` asks the matcher whether `array` contains every element of `result`, so
the assertion passes.

**Generated Rust** (`dist-smelt/src/sampleSize_spec.rs:21-28`, abridged where
noted):

```rust
    _smelt_tmp_4 = SmeltRecord::from([]);
    _smelt_tmp_5 = SmeltUnknown::Object(SmeltObject::from_unknown_record(_smelt_tmp_4.clone()));
    _smelt_tmp_6 = { let smelt_source_value = (match _smelt_tmp_5.clone() {
        SmeltUnknown::Object(map) => smelt_get_object_field(&map, "arrayContaining"),
        _ => SmeltUnknown::Undefined }.clone()); … /* no function found */
        else { ::std::rc::Rc::new(move |arg0: &SmeltList<f64>| -> SmeltUnknown { SmeltUnknown::Null }) } };
    _smelt_tmp_7 = (_smelt_tmp_6)(&result);
    _smelt_tmp_8 = !({ /* array erased to SmeltUnknown::Array */ } == _smelt_tmp_7);
```

`expect` in a *value* position lowered to an **empty record**
(`SmeltRecord::from([])`); the member read `arrayContaining` therefore yields
`SmeltUnknown::Undefined`, no callable is found, and the emitted default callback
returns `SmeltUnknown::Null`. So the assertion degenerates to
`SmeltUnknown::Array([1,2,3]) == SmeltUnknown::Null` → always false → throw.

**Root cause / layer: frontend-ts test harness.** Asymmetric matchers are not
modeled at all: `grep -rn "arrayContaining\|objectContaining\|stringContaining\|asymmetric"
crates/smelt-frontend-ts/src` returns nothing. `crates/smelt-frontend-ts/src/lowering/testing/matchers.rs`
recognizes only `expect(actual).<matcher>(expected)` — `TestMatcher::from_name`
(`crates/smelt-frontend-ts/src/lowering.rs:40-55`: `Be, Equal, StrictEqual,
Contain, HaveLength, HaveProperty, BeInstanceOf`) plus the `toHaveBeenCalled*`
family — and lowers the **expected argument as an ordinary value**
(`matchers.rs:290-300`). `expect` is only ever recognized as a *callee*
(`matchers.rs:187/280/484/578`, `stdlib.rs:735`); as a receiver it falls into the
generic erased-object member-call path.

**Shares a root with**: no other test in the 45 uses `expect.<matcher>`
(`grep -c` over the failing spec files), so this is a standalone family — but it
is the entry point for every asymmetric matcher (`expect.any`, `expect.anything`,
`expect.objectContaining`, `expect.stringContaining`, `expect.stringMatching`,
`expect.closeTo`, `expect.not.*`), and vitest allows them **nested** inside a
`toEqual` object/array literal and inside `toHaveBeenCalledWith`.

**Verdict: (a) general defect, fixable. Size M–L** (M for the flat case in this
test; L to cover nesting).

Fix design — model a matcher as a *value* with a runtime marker, exactly as
`vi.fn` mocks (`__smelt_vitest_mock`) and `Map`/`Set` (`__smelt_map`,
`__smelt_set`) already are, rather than as a special case inside `toEqual`:

1. Frontend (`lowering/testing/matchers.rs`): recognize a call whose callee is
   `<test-builtin expect>.<name>` (and `expect.not.<name>`) for the asymmetric
   matcher names, and lower it to a new
   `ExprKind::VitestAsymmetricMatcher { kind, sample, inverted }` typed
   `Unknown`. `expect` used as a value stops falling through to the empty-record
   path.
2. Codegen/runtime: erase that expr to a marker object
   `{ __smelt_asymmetric: "<kind>", sample: <erased sample>, inverted: <bool> }`,
   and add one prelude helper `smelt_asymmetric_match(actual, matcher) -> bool`
   implementing the per-kind predicates.
3. Make the **deep-equality used by `toEqual`/`toStrictEqual`/
   `toHaveBeenCalledWith` consult the marker on either side** before falling back
   to structural comparison. This is the load-bearing part: `toEqual` currently
   emits a bare `==` on two erased `SmeltUnknown`s (`sampleSize_spec.rs:27`), so
   the matcher-aware compare must be a runtime helper, not `PartialEq`
   (marker-awareness must not leak into `SmeltUnknown::eq`). Routing `toEqual`
   through that helper is what makes *nested* matchers work for free.

Regression test shape: harness tests for
`expect([1,2,3]).toEqual(expect.arrayContaining([2,1]))`,
`expect([1]).not.toEqual(expect.arrayContaining([9]))`,
`expect({a:1,b:2}).toEqual(expect.objectContaining({a:1}))`,
`expect({a:1}).toEqual({ a: expect.any(Number) })` (nesting), and
`expect(fn).toHaveBeenCalledWith(expect.anything())`.

---

## Summary

| test | root family | verdict | size |
| --- | --- | --- | --- |
| `isJSON::returns_false_if_the_value_is_not_a_valid_json_string` | throwing stdlib rvalue has no unwind edge: `JSON.parse` emitted as panicking `.expect`, `catch` arm DCE'd (`emitter/map.rs::json_parse_text`, `mir/lower/expr.rs` `ExprKind::JsonParse`) | (a) general defect | M |
| `isJSONValue::isjsonobject_should_return_false_for_not_valid_value` | `undefined` erased as `SmeltUnknown::Null` on the Dict path — `Constant::Undefined` recovery exists for `Rvalue::List` but not `Rvalue::Dict` (`emitter/coercion.rs`) | (a) general defect | S–M |
| `isJSONValue::isjsonobject_should_return_false_when_key_is_not_a_string` | `Reflect.ownKeys` aliased to `Object.keys`, symbol keys filtered out and `keys: List<String>` const-folds `typeof key !== 'string'` to `false` (`lowering/stdlib/objects.rs::object_projection_call`) | (a) general defect | M |
| `escapeRegExp::should_escape_values` | (i) JS char class with a bare `[` fails `fancy_regex` compile and silently no-ops (`rust_regex_pattern_text`, `SmeltRegExp::try_compiled`), (ii) JS replacement patterns `$&`/`$n`/`$$` never expanded (`SmeltRegExp::replace_string`) | (a) general defect | M |
| `randomInt::generates_a_random_integer_between_0_inclusive_and_max_exclusive` | source-level `!` lowered as a real `Optional → T` unwrap; `Some(x.expect(…))` into an `Option` parameter (`non_null_assertion_value` + `emitter/call_runtime.rs`) | (a) general defect | S |
| `sampleSize::returns_a_sample_element_array_of_a_specified_size` | vitest asymmetric matchers unmodeled: `expect` as a value lowers to an empty record, matcher call yields `SmeltUnknown::Null`, `toEqual` stays a structural `==` (`lowering/testing/matchers.rs`) | (a) general defect | M–L |

No test in this group is out of scope: none needs DOM, cross-realm `node:vm`,
Node `Buffer`, global monkey-patching, or `vi.spyOn` on host internals.
