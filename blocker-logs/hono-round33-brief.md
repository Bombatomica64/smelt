# Round 33 — the last 9 (orchestrator brief)

Integration head: `claude/estoolkit-test-failures-4fuf9e` @ `b291fc07` (round 32 merged). Hono
(ref `eebdf7be…` + `.github/compat/hono/.`, clean clone, fresh full-feature binary, repo-root
`--manifest-path`): 33 modules, **9 errors**. Round 30 started at 329.

| # | error | family | owner |
| --- | --- | --- | --- |
| 1 | E0425 `dispatch` not in scope (`compose.rs:12`) | a nested function declaration hoists within its enclosing closure and may reference itself; design in `hono-round32-emitter.md` §Item 4 (two halves: lower before the first statement that mentions it, and bind the name inside its own body via a recursive `Rc` knot) | **Agent I** |
| 2 | E0425 `__smelt_fn_value_627` (`main.rs:10961`) | a synthesized function-value name is minted at the definition scope and referenced from another; make the name's scope and the reference's scope one decision (`hono-round32-emitter.md` §"Also diagnosed") | **Agent I** |
| 3 | E0609 `router` on `Hono` (`main.rs:49610`) | `import { Hono as HonoBase }; class Hono extends HonoBase`: `class_extends_clause` interns the LOCAL spelling of the base; every base-chain walk must key on the resolved symbol, and the in-progress registry must answer only for symbols the current module declares (the stack-overflow trap is documented in `hono-round32-generics.md` §Item 3; H61 family) | **Agent I** |
| 4 | 3 × E0308 `Rc<dyn Fn(String, String, &(T…))>` vs by-value (`main.rs:49609`) | a default/rebuilt callable at a substituted type argument is constructed at the substituted by-value ABI while the slot keeps the declaration's by-reference ABI — the construction-side sibling of round 31 item 3 (`hono-round31-generics.md` §Found 1, `hono-round32-generics.md` §"a record rebuilt at a substituted argument erases its field": same seam, `TypeSubstitution::erased()` in the record adapter) | **Agent J** |
| 5 | 2 × E0507 move out of `closure_arg_1` in an `Fn` closure (`main.rs:11023`, `:11077`) | a callback adapter that wraps a captured callable in an inner `move` closure must clone the `Rc` per invocation (`let f = closure_arg_1.clone(); move || f()`), the same clone discipline the adapter already applies to `closure_arg_0` | **Agent J** |

Also for Agent J (no Hono count, correctness): the RED base tier
`host_representation_runtime::arguments_object_carries_values_with_a_non_enumerable_length` (E0057
in its generated crate; pre-existing, `hono-round32-emitter.md` §Gates), and the record adapter
erasing a field rebuilt at a substituted type argument (`class Pair<A> { left: Cell<A> }` emits
`Cell { value: SmeltUnknown::Number(..) }` where `f64` is declared) — fix the seam once for both
item 4 and this.

Rules: `blocker-logs/implementer-brief.md` in full. Next free fixture number is **109**. General
TypeScript-semantics rules only. Report the per-code table against 9 after each landed item. When
the crate reaches **0 errors**, also run `cargo build` on it and report whether it links; do not
start on tests (phase 3 has its own brief).

Notes: `blocker-logs/hono-round33-scoping.md` (I) and `blocker-logs/hono-round33-abi.md` (J),
allowlisted.
