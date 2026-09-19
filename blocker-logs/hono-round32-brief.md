# Round 32 — the last 45 (orchestrator brief)

Integration head: `claude/estoolkit-test-failures-4fuf9e` @ `bec88f30` (round 31 merged). Hono
(ref `eebdf7be…` + `.github/compat/hono/.`, clean clone, fresh full-feature binary, repo-root
`--manifest-path`): 33 modules, **45 errors**: E0308 35, E0631 5, E0425 2, E0609 1, E0271 1.

Rules: `blocker-logs/implementer-brief.md` in full. Fixture numbers: next free is **100**; take at
commit time, list in `END_TO_END_EXAMPLES` in the same commit. General TypeScript-semantics rules
only. Report the per-code table against 45 after each landed item.

## Agent G — generic-class type parameters (≈29 errors)

1. **Elide type parameters that nothing carries** (Agent E's proposal,
   `hono-round31-generics.md` §"The alternative"): 5+5+3+2+2 `Context_1<…>` pairs, 2 `Hono_1<…>`
   pairs, 5 E0631 closure signatures, 2 `expected SmeltUnknown, found type parameter E`, and
   probably the 2 `HonoRequest` vs `HonoRequest` and 3 `Rc` vs `Rc` (verify). Orchestrator
   ruling: **do it**. The north star is a hand-writing Rust team, and such a team does not declare
   a struct parameter its data never holds; a TypeScript type parameter that reaches no
   non-phantom field is type-level plumbing (CLAUDE.md "SmeltUnknown boundaries": preserve or
   recover the shape, do not carry plumbing to runtime). The rule: compute, per generic class, the
   least fixpoint of "parameter positions that reach a non-phantom field" (a field of type
   `Context_1<E, …>` inside `Context_1` itself contributes only through positions that are
   already carried); parameters outside the fixpoint are dropped from the emitted struct, its
   impls, every reference and every instantiation; the frontend keeps the full parameter list, so
   type-checking is unchanged and only Rust arity changes. Measure the corpora before touching
   goldens: this ripples through all three (examples goldens re-snapshot by stdout-identity; the
   es-toolkit ratchet may FALL — re-snapshot; remeda/radash must stay 1789/0 and 84/0). Land the
   analysis with unit tests first (which parameters survive on a handful of shapes, including a
   parameter carried only through a method's return type — that one IS carried if the method
   body stores it, not otherwise; be explicit and document the decision), then the emission.
2. **Two instantiations of one overloaded generic at the same argument type but different type
   arguments share a result** (`hono-round31-emitter.md` §"Arrow-const return inference",
   reproduction in radash `curry.test.ts:206-224`). Key the instantiation by the chosen overload
   AND its inferred type arguments, not by the argument types alone. Then **re-land Agent F's
   reverted commit** (`git show 9dd4afea` on `origin/worktree-agent-a747bde2996f08515`: a
   `const` with no annotation takes its initializer's type, an unannotated arrow infers its return
   from its body) and confirm radash stays 84/0 and the Hono E0271 falls.
3. `router` on `Hono` (E0609, 1): diagnose against the source (`hono-base.ts`); it is likely a
   private field read through a generic subclass and belongs to this family.

## Agent H — emitter singletons (≈16 errors) and the round-31 leftovers

1. **Structural assignability to a record type** (5 `Option<ResponseInit>` vs `Option<Response>`
   + 1 `Option` vs `SmeltResponse`; `context.ts:521`): a value whose type exposes every member of
   a target record type converts by reading each target field with the same member-read rule
   `x.member` uses and building the record. The missing piece is a text-level member read
   (`emitter::place` answers only from a `Place`); add the helper, documented, and route the
   coercion seam through it. Fixture: a class instance passed where a structurally compatible
   interface/record parameter is declared, verified against Node.
2. **A union of numeric literal types is `number`**, and `Exclude<>` over one stays a literal
   union (2 `f64` vs `SmeltUnknown`; `ContentfulStatusCode`).
3. **A callback adapter converts each argument to the adapted function's own parameter type**,
   not the contextual signature's (2: `SmeltRequest`, `SmeltTypedArray` vs `SmeltUnknown`).
4. **A nested function declaration is hoisted within its enclosing closure, not out of it**
   (E0425 `dispatch` in `compose.rs`, `__smelt_fn_value_627` one level out).
5. `buffer_to_string` returns the `SmeltArrayBuffer` where `String` is declared (`buffer.rs`):
   read `utils/buffer.ts` and name the rule (a `TextDecoder`/`String.fromCharCode` shape).
6. Round-31 leftovers from `hono-round31-emitter.md`: `instanceof Promise` on a generated union
   folds to a constant `false` (same rule as union dispatch: the tag test exists for `typeof`,
   route `instanceof` against a modeled host class through it); the RED base tier
   `union_receiver_runtime::union_optional_element_read_narrows_at_runtime` (`type table does
   not contain literal operand type Unknown`, `optional_access.rs:563` — H14's family, degrade
   at the site); an object literal at a concrete-union parameter emits a `SmeltRecord` instead of
   the union arm.

Notes: `blocker-logs/hono-round32-generics.md` (G) and `blocker-logs/hono-round32-emitter.md` (H),
allowlisted.
