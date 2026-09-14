# `Array.prototype.find` with a typed arrow: the generated crate does not compile

Requested repro (standards round 4, item 6): the Hono stream could not
reproduce this from the round-2 description. Reproduced on the merged head with
the exact source and the exact wrong generated line below.

**Severity: compile break in the generated crate.** `smelt build` reports
success; `cargo build` on the emitted crate fails with four `E0308`s. It is
therefore invisible to `smelt check`/`probe` and to any gate that stops at
lowering.

## Repro

`src/main.ts`, complete:

```ts
const names: string[] = ["ada", "grace"];
const found = names.find((name: string) => name.startsWith("a"));
const missing = names.find((name: string) => name.startsWith("z"));
console.log(found);
console.log(missing);
```

`Smelt.toml` is the ordinary program manifest (`entries = ["src/main.ts"]`,
`build = false`).

```
$ smelt --manifest-path Smelt.toml build
$ echo $?
0
$ cd dist && cargo build --message-format short
src/main.rs:2635:38: error[E0308]: mismatched types: expected `SmeltUnknown`, found `bool`
src/main.rs:2640:38: error[E0308]: mismatched types: expected `SmeltUnknown`, found `bool`
src/main.rs:2646:38: error[E0308]: mismatched types: expected `SmeltUnknown`, found `bool`
src/main.rs:2651:38: error[E0308]: mismatched types: expected `SmeltUnknown`, found `bool`
error: could not compile `ti` (bin "ti") due to 4 previous errors
```

Four errors for two source calls: each predicate is emitted **twice** (once as
a standalone `Rc` binding, once inlined into the `find`), and each copy has the
same defect.

## The wrong line

This is the load-bearing one — the closure body:

```rust
let _smelt_tmp_4 = ::std::rc::Rc::new(|closure_arg_0: String, closure_arg_1: i64, closure_arg_2: &SmeltList<String>| {
    let _smelt_tmp_3: SmeltUnknown = closure_arg_0.clone().starts_with(&"a".to_owned());
    let _smelt_tmp_4: bool = match _smelt_tmp_3.clone() { SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value, /* ... */ };
    _smelt_tmp_4
});
```

Line 2 is the error: `starts_with` yields a Rust `bool`, and it is assigned to a
binding **declared `SmeltUnknown`** with no `SmeltUnknown::Bool(..)` wrap. The
compiler's own suggestion names the fix:

```
help: try wrapping the expression in `SmeltUnknown::Bool`
2635 |     let _smelt_tmp_3: SmeltUnknown = SmeltUnknown::Bool(closure_arg_0.clone().starts_with(&"a".to_owned()));
```

## What the shape says about the cause

Line 3 is the tell. The temporary is declared `SmeltUnknown`, and the very next
statement converts it back to a `bool` through the **full JavaScript
truthiness** match (`Null | Undefined => false`, `Number => value != 0.0 && ...`,
`String => !value.is_empty()`, …). So the predicate's result was decided to be
erased — hence the `SmeltUnknown` annotation and the truthiness coercion — while
the *expression* that produces it stayed concrete, and nothing inserted the
erasure adapter between them.

Two candidate fixes, and they are not equivalent:

1. **Insert the adapter** (what rustc suggests): wrap in `SmeltUnknown::Bool`.
   Compiles, but keeps a pointless erase-then-truthiness round trip for a value
   that was statically a `bool`.
2. **Do not erase at all.** `(name: string) => name.startsWith("a")` has a
   statically known `bool` result and `find`'s callback contract wants a
   boolean, so the right generated code is `closure_arg_0.starts_with(..)` used
   directly, with no `SmeltUnknown` and no truthiness match. That is also the
   `SmeltUnknown`-boundary rule's answer: the shape is knowable, so it should not
   be tagged.

Fix (2) subsumes (1) and removes the round trip; (1) alone would make the crate
compile while leaving avoidable erasure in place.

## Notes for whoever picks this up

* The callback is emitted twice per call site. Whatever inserts the erasure runs
  on both copies, so the fix belongs where the callback's result type is
  decided, not at one emit site.
* The closure signature is fully concrete
  (`|closure_arg_0: String, closure_arg_1: i64, closure_arg_2: &SmeltList<String>|`)
  and the `find` wrapper's `find_map` consumes the call in boolean position
  (`if (smelt_callback)(item.clone(), index as i64, &smelt_array)`), so the
  *contract* around the callback is already boolean. Only the body's own result
  slot disagrees.
* An untyped arrow (`names.find((name) => ...)`) was not tried here; the round-2
  note that this "does not reproduce as described" may have used that spelling.
  The typed-parameter form above reproduces every time.
