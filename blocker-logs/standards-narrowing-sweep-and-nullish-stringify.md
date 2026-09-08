# The narrowing sweep is clean; two residues, one of them systemic

Round 16 item 3: close out the narrowing-gap notes. Every shape below was run
through Node 22 and through the generated crate and compared line by line.

## The narrowing gaps are closed

`blocker-logs/standards-instanceof-narrowing.md` part (1) — an erased local not
narrowing through `x instanceof <concrete host class>`, whose fallback answered
a silently wrong value — is fixed. Re-run at this head, all four spellings the
note named agree with Node:

```ts
function isHeaders(x: unknown): x is Headers { return x instanceof Headers; }
const value: unknown = new Headers({ "content-type": "text/plain" });
if (isHeaders(value)) console.log(value.get("content-type") ?? "null");  // text/plain

const direct: unknown = new Headers({ a: "b" });
if (direct instanceof Headers) console.log(direct.get("a") ?? "null");   // b

const params: unknown = new URLSearchParams("a=1&b=2");
if (params instanceof URLSearchParams) console.log(params.get("b") ?? "null");  // 2

const re: unknown = /a(b)c/gi;
if (re instanceof RegExp) console.log(re.source, re.flags);              // a(b)c gi
```

The wider sweep over one erased parameter agrees too — `typeof x === "string"`,
`typeof x === "number"`, `Array.isArray(x)`, `x instanceof Headers`,
`typeof x === "object" && x !== null && "k" in x`, and the unmatched
fallthrough, six for six against Node:

```
s:AB  n:2.0  a:3  h:b  o:v  other
```

Round 15 added the last one that was missing: `typeof x === 'object'` on a
UNION keeps the object-kinded arms instead of erasing the value (see
`standards-hono-headers-init-resolved.md`).

## Residue 1: an erased read of a modeled host record, and why it cannot be a static blocker

`standards-instanceof-narrowing.md` part (2) asked for the erased method-read
fallback to become "a named blocker for a receiver that carries a modeled host
marker". Still reproducible — and, checked here, **not implementable as
written**:

```ts
const erased: any = new Headers({ a: "b" });
console.log(String(erased.get("a")));   // Node: b        Smelt: (empty)
console.log(String(erased.notAMember)); // Node: undefined  Smelt: undefined
```

emits

```rust
let erased: SmeltUnknown = _smelt_tmp_2.clone().into_smelt_unknown();
let _smelt_tmp_3: SmeltUnknown = smelt_get_unknown_field(&erased.clone(), "get").clone();
// no `get` in the marker record, so the optional-call adapter substitutes a
// default callback that answers SmeltUnknown::Null
```

The blocker cannot be static because at the emit site the receiver's type is
`SmeltUnknown` and carries **no marker statically** — the marker
(`__smelt_headers`) is a runtime key of the record. So nothing there can tell
"an erased read that will land on a `Headers` record" apart from "an erased
read on a plain object", where answering `undefined` is exactly right (the
second line above). A static blocker would have to reject both.

Two implementable shapes, and the first is better:

1. **Resolve the member at runtime.** `smelt_host_method(&object, name)`
   already does this — it is how `signal?.addEventListener('abort', ..)` works
   on an erased abort record — but it knows only the abort surface
   (`abort`/`addEventListener`/`removeEventListener`/`dispatchEvent`/
   `throwIfAborted`). Every modeled host class whose erased record carries a
   marker AND has a `SmeltFromUnknown` recovery (`Headers`,
   `URLSearchParams`, `Response`, `Request`, `RegExp`, the synthetic match
   classes) could resolve its modeled members the same way: recover the
   concrete value from the record, bind the member, call it. This replaces a
   wrong answer with the right one and has no blast radius — nothing that
   works today changes — but it is per-class prelude work: one bound-method
   dispatcher per class.
2. **Throw at runtime** when the record carries a host marker and the member is
   neither an own key nor a modeled member. Cheaper, and it converts a silent
   wrong value into a diagnostic — but it is a *runtime* panic in code that
   "works" today, so it needs the corpora run before it can land.

Reachability is now narrow: with round 10's `instanceof` narrowing and round
15's `typeof`-object narrowing, the ordinary spellings all keep their types.
What is left is source that erases on purpose (`as any`, an `any`-typed field),
which is why this is a residue rather than a blocker.

## Residue 2 (new, systemic): an absent value stringifies as the empty string

Found while reading the generated code above. This one is not about narrowing
and is not about host records — it is the plain stringification of a nullish
value, and it is wrong in every spelling:

```ts
const nothing: string | null = null;
console.log(String(nothing));   // Node: null       Smelt: (empty)
console.log(`${nothing}`);      // Node: null       Smelt: (empty)
const missing: string | undefined = undefined;
console.log(String(missing));   // Node: undefined  Smelt: (empty)
console.log(`${missing}`);      // Node: undefined  Smelt: (empty)
```

```rust
let nothing: Option<String> = None::<String>;
let _smelt_tmp_2: String = nothing.clone().unwrap_or_default().to_string();
_smelt_tmp_4 = "".to_owned() + &nothing.unwrap_or_default();
```

`unwrap_or_default()` is the empty string. Four sites are involved:

| site | shape |
| --- | --- |
| `emitter/types.rs:423` | `String(x)` on `Optional<Bool/Int/Float/String>` |
| `emitter/call_runtime.rs:966` | template/`+` concatenation of an `Optional` |
| `emitter/strings.rs:744` | the ERASED stringify: `SmeltUnknown::Null => String::new()` (`Undefined` is correct there) |
| `emitter/strings.rs:796` | `Optional` of an erased value: `map_or_else(String::new, ..)` |

`${x}` over an optional field or parameter is one of the commonest lines in
TypeScript, so this is a broad correctness gap rather than a corner. It was
almost certainly imported from the Python side, where `Optional` in an f-string
is a different question — but even Python renders `None` as `"None"`, not as
`""`, so no profile wants the current answer.

**FIXED in round 17 — see the section at the bottom for the ruling that was
applied and what each spelling now prints.** The paragraph below is the
question as it was asked. Smelt's `Type::Optional` and
`SmeltUnknown::Null`/`Type::None` conflate JavaScript's `null` and `undefined`,
and the two stringify differently (`"null"` vs `"undefined"`). Three options:

1. **Pick `"undefined"` for `Optional`.** Most `Optional`s come from `?:` or
   `| undefined`, so this is right most of the time, wrong for `| null`, and
   strictly better than today (which is wrong always). One-line change per
   site.
2. **Split the nullish kinds in the type table** (`Optional` vs a nullable
   `Optional`), which makes every case right and is a type-table change with
   its own blast radius.
3. **Keep the erased path's asymmetry** (`Null => ""`) and fix only the typed
   `Optional` sites, which would leave the two paths disagreeing about the same
   source value — the worst of the three.

Blast radius for any of them: every `${optional}` and `String(optional)` in
every corpus moves, so it needs the full golden regeneration plus the
es-toolkit and remeda suites in the same commit. Some currently-failing
generated tests may start passing; a handful may start failing on assertions
written against the empty string.

---

## Fixed (round 17): the spelling comes from the body's language

The ruling was neither of the two options above: **reuse the mechanism round 2
built for `console.log`.** An absent value's word is decided during MIR
lowering from the source language of the body, not guessed in codegen — a crate
can hold TypeScript and Python modules at once, and they disagree.

`MirFunction::absent` (and `MirClosure::absent`) now carries an
`AbsentSpelling`, set once per body in the two lowering constructors from the
body's own file, exactly as `BuiltinFn::ConsoleLog` already carried it per call
site. `AbsentSpelling` gained `null_text()` alongside `text()`, because the two
words are distinguishable wherever the value still has its runtime tag:

| | JavaScript | Python |
| --- | --- | --- |
| `text()` — an absent value with no tag left | `undefined` | `None` |
| `null_text()` — an explicit `SmeltUnknown::Null`, or a `Type::None` value | `null` | `None` |

Six emit sites changed, all of which answered the empty string before:

| site | shape |
| --- | --- |
| `emitter/types.rs` | `String(x)` on an optional primitive |
| `emitter/call_runtime.rs` | `+` with an optional right-hand side |
| `emitter/strings.rs` `js_string_coercion_match_text` | the erased coercion's `Null` arm (its `Undefined` arm was already right) |
| `emitter/strings.rs` | a `Type::None` operand |
| `emitter/strings.rs` | an `Optional<String>` operand |
| `emitter/strings.rs` | an `Optional<erased>` operand, and the optional-primitive/structured catch-all that rendered `${maybeNumber}` as nothing |

`Array.prototype.join` was deliberately left alone: `[null, undefined, 1].join(",")`
is `",,1"` in JavaScript, so the empty string is the RIGHT answer there, and two
of the sites that looked like the same bug are that rule.

Measured, against Node 22 (fixture `58_absent_value_stringify`):

| source | Node | Smelt now | before |
| --- | --- | --- | --- |
| `String(undefined as string \| undefined)` | `undefined` | `undefined` | `""` |
| `${maybeString}` absent | `undefined` | `undefined` | `""` |
| `${maybeNumber}` absent | `undefined` | `undefined` | `""` |
| `${nothing}` where the type IS `null` | `null` | `null` | `""` |
| `"p:" + undefined` | `p:undefined` | `p:undefined` | `p:` |
| `${maybeString}` present | the value | the value | the value |

**The one imprecision that remains, deliberately:** a value ANNOTATED
`string | null` prints `undefined`, because TypeScript's `null` and `undefined`
both intern to one absent type and this layer cannot tell the two annotations
apart. It is the same limitation `console.log` documents, and the ruling keeps
`Type::None` conflated on purpose — the proper split is `Type::Null`, tracked
as D1 in the es-toolkit plan. A value whose type IS `null` still prints `null`,
because that type survives on its own.

Three inline codegen tests were pinning the old (wrong) text and now pin the
new one; each carries a comment saying so, since a reader will otherwise read
them as a regression. No corpus output moved: es-toolkit 1055/4 unchanged with
the ratchet at +0, remeda 1789 passed at +0, radash 84 passed, and no example
golden shifted — which is also why the fixture had to be written from scratch.
