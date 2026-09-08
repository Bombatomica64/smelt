# H33 — a class calling its own method loses the call

Round 12, item 1. Fixed here. Round 11 numbered this as "a TypeScript `private`
method is called through a stub that returns null". The fix is real but the
NAME was wrong, and the correction matters because it makes the family much
larger than the note implied.

## Privacy is not the axis

The round-11 repro used `private stash(..)`, and rewriting it as `#stash(..)`
fixed it, so privacy looked load-bearing. It is not. The same 14 lines with a
plain PUBLIC method reproduce identically:

```ts
class Registry {
  entries: Record<string, string> = {};
  stash(key: string, value: string): void { this.entries[key] = value; }
  add(key: string, value: string): void { this.stash(key, value); }
}
```

```text
%3 = closure_call copy %0.field4(copy %1, copy %2)
```

`#stash` looked like a fix because `#`-names are a DIFFERENT dispatch arm
entirely — the `Expression::PrivateFieldExpression` arm added for H1 — so it
never reaches the code below. It was never evidence about privacy.

The real rule: **any `this.sibling(..)` call written inside a class method body**
lowered to a callable-field read plus a dynamic call. That is as ordinary as
TypeScript gets.

## What went wrong

`ModuleBuilder::callable_static_member_call` claims a member call whose callee
reads as a function-typed FIELD, and defers to the real method path only when
`static_member_is_concrete_class_method` says the member names an actual method.
That helper ended in

```rust
self.resolve_method(receiver_ty, method, member.span)
    .is_ok_and(|(_, item)| item.0 != u32::MAX)
```

which treats "`resolve_method` produced no `ItemId`" as "not a method". An
`ItemId` answers the question when there is one; its absence does not. A class
whose own body is still being lowered has no registered class item yet, so
`class_by_symbol` misses, `resolve_method` falls through to
`in_progress_class_method_return` — which types the call correctly but has no
item to name — and the helper answered `false`.

The union receiver had already hit the same wall and been special-cased two
lines above (a union has no single method item by design), which is the shape of
the bug: the item id was standing in for a question it cannot answer, and each
receiver kind that fails it was being patched one at a time.

## Why it was silent

Reading the call as a callable field did not merely erase it, it LOST it. The
class has no `stash` struct field — `%0.field4` names a field that does not
exist — so the emitter answered the read with a method-value stub:

```rust
let smelt_method: Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, _>> =
    Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Null));
smelt_link_function_identity_key(&smelt_method, smelt_method_identity("Registry::stash"));
```

The call returned `null` and the body never ran. No rustc error, no diagnostic.

The method's OWN body was emitted correctly the whole time, and the
prototype-carrier path a few lines below built a real closure over
`smelt_receiver.stash(..)` for the same method. So the two paths disagreed
silently: one could dispatch the method, the other replaced it with a constant.

## The fix

Ask the question being asked. The item id is consulted first and the
DECLARATION second:

```rust
if self.resolve_method(receiver_ty, method, member.span)
    .is_ok_and(|(_, item)| item.0 != u32::MAX)
{
    return true;
}
self.receiver_declares_method(receiver_ty, method)
```

`receiver_declares_method` already answered this for unions on the callback
path, and it reads the registries (`classes.methods`, populated by `set_methods`
BEFORE any method body is lowered) rather than the lowered items, so it sees an
in-progress class. The union special case is subsumed rather than kept
alongside: a union reaches the same fallback for the same reason.

Genuine callable-storage fields — a class that really does store a closure in a
field, which must stay a closure call — are excluded before this point by
`receiver_has_callable_storage_field`, so widening the method test does not
capture them.

## Evidence

`examples/typescript/end-to-end/48_sibling_method_call`, which pins both
spellings in one class (a `private` sibling and a public one) because privacy
turned out not to be the axis:

| | output |
| --- | --- |
| Node 22 (`tsx`) | `ab,ab` |
| Smelt before | `,` |
| Smelt after | `ab,ab` |

The fixture harness compiles and RUNS the generated crate and diffs stdout, so
the golden is a behavioural assertion, not a formatting one. `expected.mir` pins
the enabling condition directly — `call fn0(copy %0, ..)` where it used to read
`closure_call copy %0.field4(..)`.

Generated Rust for the whole fixture is 71 lines and reads as hand-written:

```rust
fn add(&self, value: String) -> () {
    let _ = self.stash(value.clone());
    let _ = self.keep(value.clone());
    return;
}
```
