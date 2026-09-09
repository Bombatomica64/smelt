# H60 — a renamed class's `new` of itself resolved to the OTHER module's class

Round 25, found by running H51's own reproduction. **Fixed** in the same round.

## What happened

Two modules export a class named `Node`; H51 renames one of them to `Node_1`.
Inside `Node_1`'s own method, `new Node<T>()` lowered as the *other* module's
`Node`:

```
error[E0308]: mismatched types
2907 |     let mut _smelt_tmp_7: Option<Node_1<T>>;
2919 |     _smelt_tmp_7 = _smelt_tmp_9;   // expected `Option<Node_1<T>>`, found `Node`
```

## Why

`ClassRegistry::item` reads `by_name`, seeded with every class item visible in
the crate, and a class's own item is registered only AFTER its members are
lowered. So while a class is in progress the seeded cross-module entry is the
only match for its own source spelling.

This is older than H51 — but while both classes shared ONE name symbol the
mis-resolution was invisible (the wrong item had the right name). Giving them
distinct symbols made it a generated-crate type error, which is how it surfaced.

## The rule

A class name bound in the module's own lexical scope wins over a crate-wide
item of the same spelling. `bind_scoped_type_name` is called at the top of
`class_declaration`, before members lower, so the module's own binding is
already there when the self-reference lowers; `scoped_type_name` is only ever
set when the spelling is ambiguous crate-wide (or for block-local test-suite
classes, where the lexical binding is likewise the right answer), so nothing
else moves. Explicit type arguments are lowered and carried as the class args.

`crates/smelt-frontend-ts/src/lowering/new_expr.rs`, guarded by
`build_runs_a_renamed_class_constructing_itself` (multi-module and
build-and-run: one module cannot produce a rename, and the symptom is a value).
