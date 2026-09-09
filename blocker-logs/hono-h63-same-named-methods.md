# H63 — one class's method answered for another's

Round 27, item 2. Found in round 26 while writing H56's fixture; **fixed**. A
silent wrong value: the call disappears and its destination keeps a default.

## The measurement

```ts
class First  { count = 0;              bump(key: string): void   { this.count += key.length } }
class Second { label = 'second';       bump(key: string): string { return this.label + key } }

const second = new Second();
console.log('B:' + second.bump('!'));   // JavaScript: "B:second!"
```

| line | JavaScript | before |
| --- | --- | --- |
| `first.bump('ab')` then `first.count` | `2` | `2` — correct |
| `second.bump('!')` | `second!` | **empty** |
| `const held = second.bump('?')` | `second?` | **empty** |

Distinct method names, or the same name with the same return type, were both
correct — the trigger is a same-named method whose OTHER declaration returns
`void`.

## Why

MIR is right: `%9 = call fn2(copy %1, "!")`, and `call_text` renders
`second.bump("!".to_owned())`. The text is then discarded downstream, because
the conversion from the call's source type to the destination renders a
constant when the source has no value — the documented behaviour for a `void`
source.

The source type came from `EmitContext::function_return_types`, **keyed by the
emitted Rust name**. A method's Rust name is unique only inside its own `impl`
block: `First::bump` and `Second::bump` both key `"bump"`, so the map held one
of them (chosen by `emitted_signature_priority`) and the call to
`Second::bump(): string` was converted from `First::bump()`'s `()`. The same
key backs `function_param_types`, so the argument half had the same exposure.

## The rule

The emitted-signature maps are keyed by an **emitted-signature key**:
qualified by the owning class for a method, a static method and a constructor
(`Second::bump`), and the bare Rust name for a free function — which is what
the emitted `fn` name is unique among. `emitted_signature_key_in` is one free
function used both by the crate-level map construction (before any emitter
exists) and by `FunctionEmitter::emitted_signature_key`, so builder and readers
cannot disagree.

Overload implementations that share ONE emitted Rust function still share one
key, so the priority rule that picks between their signatures is untouched.

Every reader that holds the callee's `MirFunction` now asks for the key
(`call.rs` ×6, `local_analysis.rs` ×1). The one text-keyed lookup
(`call_runtime.rs`, which has a rendered callee text rather than a function) is
left alone: it can only match a free function's name.

## Guarded by

`examples/typescript/end-to-end/83_same_named_methods_across_classes`, verified
to fail with the emitter change reverted. It covers the `void`/value pair, the
value read into a binding, a same-named method with different PARAMETER types
(the other half of the same maps), and a third class reusing the name.
