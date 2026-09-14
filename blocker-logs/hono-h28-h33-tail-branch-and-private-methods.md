# H28 re-scoped, and H33 — a `private` method never runs

Round 11, item 2. Neither is fixed here; both are numbered with what the
investigation actually found, because round 10's guess about H28 was wrong and
H33 is worse than the error count that led me to it.

## H28 — a tail `if (cond) { …; return }` loses the branch's body

Round 10 recorded this family as "a throwing call in statement position renders
as `()` where the function's `Result<..>` is expected", from its 6 × E0308. The
error is real; the cause is not what the message suggests.

Hono's `RegExpRouter#add` (`reg-exp-router/router.ts:92`) ends with

```ts
if (/\*$/.test(path)) {
  const re = buildWildcardRegExp(path)
  for (const m of methods) { … }
  for (const handlerMap of [middleware, routes]) { … }
  return
}
const paths = checkOptionalParameter(path) || [path]
for (const path of paths) { … }
```

and Smelt emits

```rust
if _smelt_tmp_52 {
} else {
    _smelt_tmp_89 = SmeltRecord::from([]);
    …the code that follows the `if`…
}
```

The then-branch is **EMPTY**: every statement inside it — two loops, the
`#insertPath` calls, the middleware writes — is gone, the continuation has been
moved into an `else`, and the function (typed
`Result<SmeltUnknown, Box<dyn Error>>`) now falls off its end, which is the
E0308 the compiler reports. So the compile error is a symptom of dropped code:
were the types to line up, this would be a silent wrong answer.

I could not reduce it. A 25-line method with the same shape — an early `throw`,
`Object.keys` versus `[method]`, a tail `if (path.endsWith('*')) { for (…)
{ … } return }` followed by another loop — lowers correctly, with both branches
populated and `return Ok(())` in each. So the trigger is something narrower in
that function: the candidates I did not get to are the `re.test(p) &&
handlerMap[m][p].push(…)` expression-statement, the
`findMiddleware(…) || findMiddleware(…) || []` chain feeding a nested index
write, and the `for (const handlerMap of [middleware, routes])` loop over an
array literal of two records.

Whoever takes it: start from the MIR of `add`, not from the generated Rust. An
empty emitted branch means the structured-control-flow reconstruction lost a
block, so the question is whether MIR has the statements at all.

## H33 — a TypeScript `private` method is called through a stub that returns null

> **Superseded — see `hono-h33-sibling-method-call.md`.** The bug below is real
> and is fixed, but the diagnosis in this section is wrong on one point:
> privacy is NOT the axis. A plain PUBLIC sibling method call reproduces it
> identically, so the family is every `this.sibling(..)` call written inside a
> class method body, not the `private` spelling. `#stash` appeared to fix it
> only because `#`-names take a different dispatch arm entirely.

Found twice while working on H25 and H28, and reproduced in 27 lines:

```ts
class Registry {
  entries: Record<string, string> = {};

  private stash(key: string, value: string): void {
    if (key === '') { throw new Error('empty key'); }
    this.entries[key] = value;
  }

  add(method: string, path: string): void {
    const methods = method === 'ALL' ? Object.keys(this.entries) : [method];
    if (path.endsWith('*')) {
      for (const m of methods) { this.stash(m, path); }
      return;
    }
    for (const m of methods) { this.stash(m, path + '!'); }
  }
}
const registry = new Registry();
registry.add('GET', 'a*');
registry.add('POST', 'b');
console.log(registry.entries['GET'] + ',' + registry.entries['POST']);
```

| | output |
| --- | --- |
| Node 22 (`tsx`) | `a*,b!` |
| Smelt (`cargo run`) | `,` |

The method body IS emitted (`fn stash(&self, key: String, value: String) -> Result<(), Box<dyn Error>>`),
but the CALL does not reach it. `this.stash(..)` is lowered as an erased
method-VALUE read plus a dynamic call, and the value it reads is a stub:

```rust
let smelt_method: Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, _>> =
    Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Null));
smelt_link_function_identity_key(&smelt_method, smelt_method_identity("Registry::stash"));
```

so the call answers `Null` and the writes never happen. No rustc error.

Renaming the method changes nothing (`insert` → `stash`: same stub). Changing
the PRIVACY SPELLING fixes it completely: `#stash(..)` with the identical body
and signature emits `self._stash(..)` at both call sites and prints `a*,b!`.
Both spellings are the same language feature, so whatever method table the direct
call path consults does not contain TypeScript-`private` methods, while the
prototype-carrier path does — and the two disagree silently.

This is the second family this round with no diagnostic (H31 was the first), and
unlike H31 it is not a corner: `private` is how most TypeScript codebases spell a
helper method.
