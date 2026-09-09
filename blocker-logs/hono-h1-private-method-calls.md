# Hono family H1 — `call expression is not lowered yet`: ES private-name method calls

Probe: `smelt --manifest-path third_party/hono/Smelt.toml check --message-format json`
at `honojs/hono@eebdf7be39abf0a872671835ccce0c4f03ea497a`.
4 occurrences, 4 files, all one shape.

## 1. The sites

| file | line | source |
| --- | ---: | --- |
| `src/context.ts` | 795 | `this.#notFoundHandler(this)` |
| `src/router/reg-exp-router/router.ts` | 82 | `this.#insertPath(method, p)` |
| `src/router/reg-exp-router/prepared-router.ts` | 65 | `this.#addWildcard(m, handlerData)` |
| `src/router/trie-router/node.ts` | 113 | `this.#pushHandlerSets(handlerSets, nextNode.#children['*'], method, node.#params)` |

Every one is a call whose callee is an **ES private name**: `recv.#method(args)`.
Nothing about Hono is load-bearing — the shape is a class calling one of its own
`#`-prefixed methods, which is how any TypeScript codebase written after ES2022
spells a non-public helper.

## 2. Wrong output

There is no wrong output: lowering **rejects** the file. Reduced to 14 lines:

```ts
class Counter {
  #count: number = 0
  #bump(by: number): number {
    this.#count += by          // private FIELD read/write: lowers fine
    return this.#count
  }
  add(by: number): number {
    return this.#bump(by)      // private METHOD call: "call expression is not lowered yet"
  }
}
```

`smelt dump-hir` on that file reports exactly the Hono diagnostic, spanning
`this.#bump(by)`. So the private *field* half of the feature was already
complete (`private_field_member` in
`crates/smelt-frontend-ts/src/lowering/stmt/assignments.rs:2805` reads
`this.#count` through `class_field_type`) and only the *call* half was missing.

## 3. Responsible function

`ModuleBuilder::call_expression`,
`crates/smelt-frontend-ts/src/lowering/stdlib/call_dispatch.rs:67`.

It is a chain of `if let` arms over the callee's AST shape. The generic
member-call arm was

```rust
if let Expression::StaticMemberExpression(member) = &call.callee {
    ... resolve_method(...) -> ExprKind::Method { receiver, method, args }
}
```

and there was **no arm for `Expression::PrivateFieldExpression`**. A private
call therefore fell past `local_callable_call`, the `NewExpression` callee arm,
and the identifier-callee arm, reaching the terminal

```rust
Err(SmeltError::unsupported(span, "call expression is not lowered yet"))
```

at `call_dispatch.rs:947`.

Nothing deeper was missing. `resolve_method`
(`crates/smelt-frontend-ts/src/lowering/ty/annotations.rs:2820`) looks a method
up in `class.methods` by interned symbol, and class lowering already registers a
private method under its **bare** source name — `PropertyKey::PrivateIdentifier`
at `crates/smelt-frontend-ts/src/lowering/decls/functions.rs:2140` and `:2667`
yields `identifier.name`, which in oxc excludes the `#` sigil. The same is true
of `private_field_member`, which interns `member.field.name`. So the two
namespaces (public property names and private names) are already flattened into
one symbol space *below* the dispatch; only the dispatch itself distinguished
them.

## 4. Design

Private calls are not a new feature; they are the existing member call reached
through a second spelling. Duplicating the ~80-line arm would have left two
copies to drift apart (the optional-access desugar, the erased-callback
argument rule, the `ItemId::MAX` dynamic-boundary fallback). So the arm was
extracted into one documented helper and both spellings call it:

```rust
fn member_call(
    &mut self,
    call: &CallExpression<'_>,
    object: &Expression<'_>,
    property_name: &str,       // no `#` sigil, either spelling
    member_span: Span,
    member_optional: bool,
    body: &mut Body,
) -> Result<ExprId, SmeltError>
```

`StaticMemberExpression` passes `member.property.name` / `member.optional`;
`PrivateFieldExpression` passes `member.field.name` / `member.optional`.

Two placement decisions:

* The private arm sits immediately **after** the static arm, i.e. after the
  builtin/namespace/static pre-passes (`dispatch_builtin_call`,
  `global_alias_namespace_call`, `callable_static_member_call`,
  `class_static_method_call`). That is sound rather than incidental: a private
  name cannot denote a builtin namespace, a global alias, or a static stdlib
  member, because private names are only resolvable inside the class body that
  declares them. Consulting those pre-passes for a private callee could only
  ever produce a false positive.
* The `property_name == "next"` iterator arm and the `property_name == "test"`
  erased-callback argument arm stay in the shared helper rather than being
  gated to the public spelling. Both are already *name*-driven, not
  spelling-driven, and neither can fire for a private call (a `#next` on a
  `Type::List` receiver and a `#test` regex receiver are both unconstructible),
  so keeping them shared costs nothing and avoids a second behavioural fork.

No type is erased and no `SmeltUnknown` is introduced: a private call resolves
to a concrete `ExprKind::Method` against a concrete `Type::Class` with the
declared return type, exactly like its public twin.

## 4b. The second layer: a private read in ARGUMENT position

Fixing the callee exposed a second, independent gap in the same feature. With
the call itself lowering, `context.ts` and `trie-router/node.ts` reported

```text
call argument kind is not lowered yet: PrivateFieldExpression(..)
```

for `this.#pushHandlerSets(handlerSets, nextNode.#children['*'], method, node.#params)`
— a private field read used as a call **argument**. `Argument` is a separate oxc
enum from `Expression`, and `ModuleBuilder::argument`
(`crates/smelt-frontend-ts/src/lowering/guards.rs:1214`) enumerates its variants
independently: it had `Argument::StaticMemberExpression -> static_member` but no
`Argument::PrivateFieldExpression` arm, so it fell to its own terminal error at
`guards.rs:1360`. The fix delegates to `private_field_member`, exactly as the
expression position already did (`new_expr.rs:2533`), so the two positions
cannot diverge.

This is the general shape of the whole family: private names are the *same*
member access under a second AST spelling, and every position that enumerates
AST variants needs the delegating arm. Two positions were missing it (callee,
argument); the expression and assignment-target positions already had one.

## 5. Generality

The rule fires for any `recv.#m(args)`, from any source, including on a receiver
other than `this` — private names are class-scoped, so `other.#m()` inside the
class body is legal and is the shape `trie-router/node.ts:113` writes. It does
not mention Hono, a file, or a method name.

## 6. Tests

* `crates/smelt-frontend-ts/src/tests/class_module_tests.rs`
  — `lowers_private_class_method_call` (the reduced repro) and
  `lowers_private_class_method_call_on_another_instance` (cross-instance
  receiver). Both assert the module lowers and `smelt_hir::validate` is clean.
* `crates/smelt-codegen-rust/tests/private_member_call_runtime.rs` (new tier,
  registered in the `functions` shard of `.github/workflows/runtime-tiers.yml`)
  — four fixtures that COMPILE AND RUN: accumulated state across calls, full
  argument order/arity, a private call on another instance, and private
  recursion plus sibling private calls. Compiling alone would not catch a
  member-call path that lowered the wrong receiver or dropped arguments, which
  is the plausible failure mode for a call that was previously rejected.

## 7. Result

4 occurrences -> 0. The four Hono files no longer report this class.
