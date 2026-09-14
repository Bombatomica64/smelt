# H61 — a module's own class lost to another module's class of the same name

Round 25, found by re-measuring the router slice after H60. **Fixed** in the
same round, and it is what unblocked the slice.

## The stop it caused

```
Error: optional record field `#pattern` is unknown on Node_1
  (in `insert` at third_party/hono/src/router/trie-router/node.ts:766..1681)
```

Enriching that message temporarily with the receiver's known fields named the
culprit outright:

```
[DEBUG known: Some(["#index", "#var_index", "#children"]) inner=Class { name: Symbol(21), args: [] }]
```

`#index`/`#varIndex`/`#children` are the **reg-exp** router's `Node`. The trie
router's `insert` was reading fields on the reg-exp router's class.

## Why

H51 renames an ambiguous class name per module (`Node`, `Node_1`, ...), and the
LAST declaring module keeps the bare spelling so existing goldens stay
byte-identical. Only the renamed modules got an entry in the rename map, so
only they bound the name in their own scope
(`ClassRegistry::bind_scoped_type_name`).

For the module that keeps the bare name, `resolve_type_reference_symbol` fell
through to the crate-wide by-name item map — whose entry for an ambiguous
spelling is whichever module registered last. Hono's trie router keeps the bare
`Node`, so its own `let curNode: Node<T> = this` and its `Record<string,
Node<T>>` field resolved to the reg-exp router's class.

This was invisible before H51 (one symbol for both classes, so the wrong item
still had the right name) and it is the second half of the same defect as H60:
a class's own module must win for its own spelling, whether it reaches the name
through a type annotation (H61) or through `new` (H60).

## The rule

`manifest_class_renames` now maps EVERY module that declares an ambiguous name,
including the winner — whose entry maps the name to itself. The Rust name is
unchanged, so nothing about any existing generated crate moves; what changes is
that the frontend now binds the name in the declaring module's own scope, and a
module's own class wins for its own spelling.

The map is therefore also the answer to "does this module declare this
ambiguous name", which is the fact the frontend actually needs.

Guarded by `build_runs_a_module_referring_to_its_own_ambiguous_class_name`
(build-and-run, two modules, the bare-name winner reading its own private
field). Verified to fail with the change reverted.

## What it unblocked

The Hono **router slice transpiles for the first time**: 18 generated files,
`cargo check` reaching 57 errors (50 E0308, 2 E0424, 2 E0277, 1 E0615 — H44's
own error is back and now reachable, 1 E0283, 1 other). Before this it did not
transpile at all.
