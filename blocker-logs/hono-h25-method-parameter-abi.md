# H25 — a class-method call ignored the parameter ABI it was calling

Round 11, item 2, first family: **419 of the router+utils slice's 498 errors**,
and one rule.

## The shape

Hono's trie router has a private method that pushes into its first parameter and
takes an optional tail:

```ts
#pushHandlerSets(
  handlerSets: HandlerParamsSet<T>[],
  node: Node<T>,
  method: string,
  nodeParams: Record<string, string>,
  params?: Record<string, string>
): void {
  ...
  handlerSets.push(handlerSet)
  ...
}
```

`parameter_needs_mutable_reference_in` sees the in-place mutation and emits the
parameter by reference, exactly as a hand-written Rust team would:

```rust
fn _push_handler_sets(&self, mut handler_sets: &mut SmeltList<SmeltUnknown>, ..)
```

The CALL SITES then contradicted the signature:

```rust
self._push_handler_sets(handler_sets.clone(), child.clone(), ..)
//                      ^ expected `&mut SmeltList<SmeltUnknown>`,
//                        found `SmeltList<SmeltUnknown>`     (E0308 ×400)
self._push_handler_sets(handler_sets.clone(), node, method, params)
//   ^ this method takes 5 arguments but 4 arguments were supplied (E0061 ×19)
```

## Why

`Callee::Static` has four arms in `emitter/call.rs`, one per `HirOrigin`. The
free-function arm runs the full argument ladder (`emitter::static_call_args`), and
the static-method and constructor arms pad omitted trailing parameters. The
INSTANCE-METHOD arm did neither: it mapped every argument through
`callee_generic_argument_text` and stopped at the last written argument. So the
one callee kind whose parameters are the most likely to be mutated in place — a
method, which is where a private helper that fills a caller's collection lives —
was the one that never asked whether a parameter was by-reference.

Nothing about the ABI is method-specific, which is the point: the predicate that
DECIDED the signature is the same one the call site can consult.

## The rule

A class-method call obeys the same two rules as every other call:

* a parameter the callee mutates in place is passed as a mutable borrow
  (`&mut sink`), not as a value — `parameter_needs_mutable_reference_in`, the
  predicate that emitted the `&mut`, decides it;
* a parameter the call omits is padded with its default, so an optional tail
  (`params?: Record<string, string>` → `Option<SmeltRecord<..>>`, default `None`)
  does not leave the call short.

A by-reference parameter is skipped rather than padded when it has no argument:
`&mut T` has no value-shaped default to invent, and such a parameter is only
reachable through an explicit argument. That is the same rule, for the same
reason, as `indirect_call_args_text`.

## Effect on the slice

| | errors |
| --- | ---: |
| before | 498 |
| after | **79** |

The removed 419 are exactly this family: 400 × E0308 (`&mut` versus value) and
19 × E0061 (the omitted optional tail).

## Test

`crates/smelt-codegen-rust/src/tests/static_call_arg_precedence_tests.rs` —
`a_method_call_borrows_a_mutated_parameter_and_pads_an_omitted_one`: a private
method with a mutated array parameter and an optional trailing parameter, called
once with the tail and once without. It asserts the signature really is `&mut`
first (so the call-site assertions cannot pass vacuously), then that the call
borrows rather than clones, pads `None::<String>` where the tail is omitted, and
still passes `Some("twice")` where it is written.

## Found while writing that test — H33, numbered, not fixed

Spelling the same helper `private addRow(..)` instead of `#addRow(..)` makes the
CALL disappear. `this.addRow(sink, row)` is emitted as an erased receiver-bound
method value whose body is a stub:

```rust
let smelt_method: Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, _>> =
    Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Null));
```

so the method never runs and the call answers `Null` — no rustc error, like H31.
A `#`-private method with the identical body and signature lowers to the direct
`self._add_row(..)` call this note is about. The two privacy spellings must
lower the same way; only one of them does today.
