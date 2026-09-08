# `/regex/.test(s)` erases a match it only null-checks

**STATUS: FIXED (standards round 14).** `test` now lowers to one of two nodes,
neither of which builds a match object; the section "How it was fixed" at the
bottom records what shipped, and
`examples/typescript/end-to-end/54_regex_test_predicate` is the fixture.

Found while writing `examples/typescript/end-to-end/51_web_crypto`. One
avoidable erasure per call site, in one of the commonest shapes in JavaScript.

## Reproduction

```ts
const id = "0a1b";
console.log(/^[0-9a-f]{4}$/.test(id));
```

## What is emitted

```rust
_smelt_tmp_27 = SmeltRegExp::new("^[0-9a-f]{4}$".to_owned(), "".to_owned());
_smelt_tmp_28 = _smelt_tmp_27.exec(&id).map(SmeltMatch::into_smelt_unknown);
_smelt_tmp_29 = !(_smelt_tmp_28.as_ref().map_or(true, |value|
    matches!(value, SmeltUnknown::Null | SmeltUnknown::Undefined)));
```

Three things go wrong at once: the call runs `exec`, which BUILDS a match
object; the match is erased to `SmeltUnknown` purely so it can be compared
against null; and the answer — a `bool` — is then recovered from that erased
value. A hand-writing Rust team would emit `regex.is_match(&id)`.

`smelt smelt-unknown-report` classifies the two `SmeltUnknown` lines as
avoidable erasure, which is what makes this block the examples-corpus invariant
(`avoidable == 0`) for any fixture that uses the shape.

## Why the recognized rule does not fire

`RuleId::TsRegExpTest` exists and the recognition table already matches a regex
literal receiver:

```rust
// crates/smelt-frontend-ts/src/lowering/stdlib_dispatch.rs
Expression::RegExpLiteral(_) if property == "test" => Some(RuleId::TsRegExpTest),
```

so `call_rule` answers the right rule. `exact_stdlib_call` does not handle
`TsRegExpTest`, and the handler that does claim the call earlier in the builtin
chain lowers `test` as `exec(..) != null` instead of to `ExprKind::RegexIsMatch`.
`new RegExp(..).test(..)` and `RegExp(..).test(..)` reach the same handler, so
they are worth checking together — the erasure may not be literal-specific.

## Where to fix it

The `test` lowering should produce `RegexIsMatch` for every receiver spelling
the recognition table accepts. `RegexIsMatch` already exists in HIR/MIR and
already emits `is_match`, so this is a dispatch fix rather than a new operation.

## Blast radius

Every `.test()` call in the corpora. Fixing it should REDUCE avoidable erasure
in es-toolkit and remeda, so it wants the ratchet baselines re-snapshot in the
same commit.

## How it was fixed

`test` lowers to one of TWO nodes, chosen by whether the receiver's flags are
readable at the call site, because `RegExp.prototype.test` is two different
operations:

* **Stateless** — a regex literal without `g`/`y`, or a `new RegExp(p)` /
  `RegExp(p)` built with no flags argument. Flags are readable here and none of
  them is stateful, so the call is a pure predicate and lowers to the existing
  `ExprKind::RegexIsMatch { op: Search }`, which emits `is_match`.
* **Stateful** — everything else, including a regex reached through a name (a
  `const`, an object field) and a `new RegExp(p, flags)`. `test` on a `g` or `y`
  regex READS and ADVANCES `lastIndex`, and whether the value carries those
  flags is a property of the value, not of the expression. These lower to a new
  `ExprKind::RegexTest { regex, haystack }` typed `bool`, which emits
  `SmeltRegExp::test` — the runtime's own method, which performs the same search
  `exec` does and updates `lastIndex` with it.

`RegexIsMatch` could not serve the second case and `RegexExec` could not serve
either without erasing its result, which is why the new node exists.

Two things had to be right beyond the dispatch:

* **`new RegExp(p, flags)` stays stateful even when `flags` names no `g`/`y`.**
  `regexp_pattern_expression` answers a pattern STRING, and its constructor arm
  takes only the pattern argument — the flags are dropped. Taking the predicate
  path there made `new RegExp("^a+$", "i").test("AAA")` answer `false` where
  Node answers `true`. Widening the fast path to flagged constructions needs
  that arm to fold the flags into the pattern first.
* **`Rvalue::RegexTest` had to join the regex prelude demand.** It is the one
  RegExp operation whose result type mentions nothing regex-shaped, so a program
  whose only regex use is a `test` was emitting `SmeltRegExp::new(..)` against an
  empty prelude.

One pre-existing runtime bug surfaced while diffing the fixture against Node: a
FAILED match on a `g`/`y` regex did not reset `lastIndex` to 0 in the generated
`SmeltRegExp::exec`, so a global regex never recovered after exhaustion (a
fourth `test` answered `false` where Node answers `true`). Fixed in the same
commit.

## Measured effect

es-toolkit avoidable erasure 32518 → 32493 (−25), remeda 25065 → 25028 (−37);
both baselines re-snapshot in the same commit. The examples corpus stays at 0
avoidable with the new fixture (stateful and stateless cases) in it.
