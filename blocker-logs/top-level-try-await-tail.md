# A top-level `try` that awaits emits a tail it cannot compile

> **FIXED in round 14.** Each arm of the emitted `match` now gets its own Rust
> declaration scope, which is the rule `emitter::closures` already followed and
> these three sites did not. Fixture:
> `examples/typescript/end-to-end/53_top_level_try_tail`.

Found while writing `examples/typescript/end-to-end/51_web_crypto`. Not a
`WebCrypto` bug: any awaited call inside a module-top-level `try` reproduces it.

## Reproduction

```ts
async function work(): Promise<number> {
  return 1;
}

let name = "no throw";
try {
  await work();
} catch (error) {
  name = (error as Error).name;
}
console.log(name);
```

## What is emitted

An `await` inside `try` becomes a `match fut.await { Ok(..) => .., Err(..) => .. }`,
and the statements AFTER the try/catch are duplicated into both arms. At module
top level those statements include the synthesized exit drain, and the
duplication is not symmetric: the `Ok` arm gets the `let` declaration, the `Err`
arm gets a bare assignment to a name that arm never declared.

```rust
match _smelt_tmp_61.await {
    Ok(__smelt_value) => {
        // ...
        let _smelt_tmp_65: SmeltFuture<()> = SmeltFuture::from_future(/* exit drain */);
        _smelt_tmp_66 = _smelt_tmp_65.await?;
        return Ok(());
    }
    Err(__smelt_error) => {
        // ...
        _smelt_tmp_65 = SmeltFuture::from_future(/* exit drain */);   // no `let`
        _smelt_tmp_66 = _smelt_tmp_65.await?;
        return Ok(());
    }
}
```

```
error[E0425]: cannot find value `_smelt_tmp_65` in this scope
```

## Why it only shows at top level

Inside a function body the same source compiles: the temporaries the tail needs
are declared in the function's own prologue, above the `match`, so both arms
assign to names already in scope. It is the module entry's tail — the exit drain
appended after the last statement — that is emitted with an inline `let` in one
arm only. `crates/smelt-codegen-rust/tests/web_crypto_runtime.rs` puts its
try/await case in a test function for exactly this reason, and it passes there.

## Where to fix it

The declaration of a tail temporary belongs in the enclosing scope's prologue,
not inside whichever arm happens to be emitted first — the same rule the
function-body path already follows. `emitter::control_flow`'s try/catch arm
emission and the module-entry tail are the two sites.

## Blast radius

Any `try { await .. } catch { .. }` at module top level, which is an ordinary
shape in scripts and in Hono/Express entry files. It fails loudly (the crate
does not compile) rather than silently, so it is a blocker rather than a
correctness risk.

## What the fix was

`emit_continuation` emits the SAME join block once per arm, and each arm is its
own Rust lexical scope — but the "already declared" set was function-wide, so
the first arm emitted the `let` and the second assigned to a name its scope had
never declared. `declared_locals_snapshot`/`restore_declared_locals` already
existed for exactly this, and `emitter::closures` already snapshots around each
of its arms; the three sites in `emitter::control_flow` — the throwing-call
terminator, its `Ok(Ok)`/`Ok(Err)`/`Err` three-arm sibling, and the awaited
rejecting future — did not. They do now, restoring before each subsequent arm
and after the last so an arm's locals do not leak out either.

It was never specific to `await`: any module-top-level `try` whose tail declares
a temporary reproduced it. The `await`s made it easy to hit because the module
entry's exit drain is itself part of that tail.
