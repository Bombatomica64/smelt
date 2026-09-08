# A module-level arrow lifted to a named function loses its captures

**Class:** general lowering (not stdlib, not host surface). Found while diffing
the `node:events` `EventEmitter` probe against Node 22; the emitter was
incidental and is not needed to reproduce.

**Symptom:** silently wrong output. Nothing fails to compile, nothing reports a
blocker, and the generated Rust runs — it just answers a different value than
the source does.

## Repro

```ts
const rem: string[] = [];
const second = () => { rem.push("second"); };
const outer = () => { rem.push("first"); take(second); };
function take(f: () => void): void { f(); }
outer();
console.log(rem.join(","));
```

Node 22 prints `first,second`. Smelt's generated crate prints `first`.

The trigger is narrow and worth stating exactly: a **module-level** `const`
bound to an arrow, whose body reads another module-level binding, and which is
**referenced from inside another closure** rather than called directly. Calling
`second()` at module level instead lowers correctly; so does the same pair of
arrows declared inside a function body. Both of those were checked.

## What the MIR shows

```
fn second__module_src/main.ts (FuncId(1)) -> None
  locals
    %0 temp: List<String>
    %1 temp: Float
  bb0:
    %0 = []                       <-- a FRESH list, not the module's `rem`
    %1 = list_push move %0, "second"
    return none

fn main (FuncId(2)) -> None
  ...
  %2 = []
  %0 = move %2                    <-- the module's real `rem`
  %3 = closure ClosureId(0) [copy %0]
  ...

closure ClosureId(0) -> Unknown   <-- `outer`, captures `rem` correctly
  bb0:
    %1 = list_push copy %0, "first"
    %2 = closure ClosureId(1) []  <-- a capture-free thunk for `second`
    %3 = call fn0(copy %2) -> bb1
```

Two things went wrong together:

1. `second` was **lifted out of the module body into a named MIR function**
   (`second__module_src/main.ts`) instead of staying a closure. The other arrow
   in the same file, `outer`, stayed `ClosureId(0)` and captured `rem` the way
   it should — so the lift is not applied uniformly.
2. Because the lifted form is a plain function with no capture environment, the
   initializer of the binding it reads was **re-materialized inside it**:
   `%0 = []` builds a second, private empty list, and every `push` lands there.
   The module's `rem` never sees the write.

The second half is the damaging one. A lifted function that simply failed to
resolve `rem` would be a compile error and therefore honest; duplicating the
initializer produces a program that runs and quietly diverges. Whatever decides
to lift a module-level arrow must either keep the capture (stay a closure, as
`outer` does) or refuse to lift.

## Not worked around

The `EventEmitter` runtime tier
(`crates/smelt-codegen-rust/tests/event_emitter_runtime.rs`,
`emit_iterates_a_snapshot_of_the_listener_list`) declares its shared listener
inside the test function rather than at module scope, which sidesteps this. That
is a scoping choice in a fixture, not a fix, and the bug above is untouched.
