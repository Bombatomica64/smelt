# Hono family H12 — calling a module const that aliases a function

Not on the plan's H1–H10 list. Found by the H10 runtime tier: the fixture that
calls an aliased global directly (`decodeURIComponent_('a%2Fb')`) failed while
the same alias passed to a higher-order function worked. Reducing it showed the
defect has nothing to do with globals.

## 1. Wrong output

```ts
function original(s: string): string {
  return s + '!'
}

export const alias = original
export const viaAlias = (s: string): string => alias(s)
```

Generated Rust (before):

```rust
pub(crate) fn via_alias(s: String) -> Result<String, Box<dyn std::error::Error>> {
    let _smelt_tmp_1: String = String::new();
    return Ok(_smelt_tmp_1);
}
```

`alias(s)` **is not called**. The function answers the empty string for every
input, compiles without a warning, and reports no diagnostic. `original` is
still emitted, so nothing looks missing. This is the dominant defect class in
this repo — a value that type-checks, compiles, and is silently wrong — and it
fires for a plain user function, not just for a global.

## 2. Responsible function

`ModuleBuilder::call_expression`'s item-callee arm,
`crates/smelt-frontend-ts/src/lowering/stdlib/call_dispatch.rs:596-620`.

When the callee identifier resolves to a module item that is **not** an
`Item::Function` — a `const` bound to a function value — but whose type IS a
function, the arm built the callee like this:

```rust
let callee = body.push_expr(Expr {
    kind: ExprKind::Literal(Literal::None),   // <- a fabricated NULL callee
    ty: item_ty,
    span: …,
});
return Ok(body.push_expr(Expr {
    kind: ExprKind::ClosureCall { callee, args },
    ty: function.return_ty,
    …
}));
```

A `ClosureCall` on a null callee typed as a function: downstream the call has
nothing to dispatch to and the result is a default-constructed value. The shape
is the "fabricated null callee" already named in the module doc of
`crates/smelt-codegen-rust/tests/truthiness_and_await_runtime.rs` ("a callback
built by calling such a factory … instead of being modeled as a fabricated null
callee"); this site still had it.

## 3. Design

Read the item's **value**. `identifier_expression` is the general "read this
name" path — the same one `local_callable_call` uses one arm below — and it
inlines a module const's initializer, so it produces the real callable whether
the const aliases a declared function, an arrow, or a builtin global value
closure:

```rust
let callee = self.identifier_expression(
    callee_ident.name.as_str(), callee_ident.span.start, callee_ident.span.end, body,
)?;
```

Nothing else changes: the surrounding `ClosureCall` and its return type were
already right; only the callee operand was fabricated.

The emitted Rust becomes an actual call:

```rust
pub(crate) fn via_alias(s: String) -> String {
    let _smelt_tmp_1 = ::std::rc::Rc::new(|closure_arg_0: String| {
        let _smelt_tmp_1: String = original(closure_arg_0.clone());
        _smelt_tmp_1.clone()
    });
    let _smelt_tmp_2: String = (_smelt_tmp_1)(s.clone());
    return _smelt_tmp_2;
}
```

The `Result` return also disappears, because the fabricated path had marked the
function as possibly throwing.

## 4. Generality

The rule is "a call through a name that resolves to a non-function item reads
that item's value". It mentions no library and no builtin; it fires for
`const alias = userFunction`, `const alias = someArrow`, and
`const alias = encodeURIComponent` alike.

## 5. Why it surfaced now

Family H10 made `export const decodeURIComponent_ = decodeURIComponent` lower
at all — before, it was rejected outright, so no call to it could exist. The
underlying defect is older and independent: the reduced repro above uses only a
plain function declaration and no global.

## 6. Tests

`crates/smelt-codegen-rust/tests/uri_transcode_runtime.rs`,
`each_global_works_as_a_first_class_value` — the case
`expect(decodeURIComponent_('a%2Fb')).toBe('a/b')` is a direct call through the
alias, which is the assertion that caught this. A value that is merely *passed*
to a higher-order function goes through a different path and did not.
