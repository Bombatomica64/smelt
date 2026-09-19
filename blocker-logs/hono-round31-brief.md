# Round 31 — the last 90 (orchestrator brief)

Integration head: `claude/estoolkit-test-failures-4fuf9e` @ `06c69924` (round 30 merged: both agents).
Hono (ref `eebdf7be…` + `.github/compat/hono/.`), measured by the orchestrator from a clean clone
with a fresh full-feature binary, **always from the repo root with `--manifest-path <abs>`** (a
cwd-relative manifest ignores `[sources] exclude`; see Agent E item 4): 33 modules, **90 errors**:
E0308 51, E0277 24, E0631 5, E0382 5, E0425 2, E0609 1, E0599 1, E0271 1. Round 30 started at 329.

Rules: `blocker-logs/implementer-brief.md` in full. Next free fixture number is **95**; take at
commit time and list in `END_TO_END_EXAMPLES` in the same commit. General TypeScript-semantics
rules only. Report the per-code table against 90 after each landed item.

## Agent E — generics across instantiation boundaries (~50 errors)

1. **A generic class's own type parameters inside its own generic scope** (≈19 E0308
   `Context_1<SmeltUnknown, …>` vs `Context_1<E, …>` in both directions, 5 E0631 closure
   signatures, 2 `expected SmeltUnknown, found type parameter E`, 2 `Hono_1<…>` pairs).
   Round 30 made an outside reference take the declaration's defaults. Inside generic code the
   rule is different: a reference to a type parameter `E` stays `E`; a reference to the class
   without arguments inside its own body means the current parameters (`this`-typed positions,
   `Context<E>` inside `class Hono<E, S, BasePath>` where `E` is in scope). The defaults are
   taken only for parameters the reference does not and cannot mention. Find where the two
   spellings meet (callback adapters — `_smelt_adapted_callback` — and the `Default` callbacks
   at `main.rs:6369`, `:10020`, `:11094` in the current emission) and make the lexical scope
   decide once, per H42's render-scope rule. Fixture: a generic class with defaulted parameters
   whose methods store and call callbacks typed over the class itself, instantiated with and
   without explicit arguments.
2. **Trait bounds on generic items that flow into a generated class** (24 E0277 + 1 E0599):
   `impl<E: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, …> Default for
   Hono_1Inner` is required from a `fn <E, S, BasePath, CurrentPath>` that declares no bounds.
   Rule: every generic item (function, closure adapter, impl) emits for each of its type
   parameters the bound set the crate uses for class parameters, from one helper, so the set
   cannot drift between the class and its users. Check `type_param_render_scope_runtime`
   and the examples corpus for any emitted `impl<T>` without bounds and unify.
3. **Substituted type parameter keeps the declaration's ABI** (round-30 Agent C leftover, the
   last `&(SmeltUnknown, RouterRoute)` E0308 and its ~15-line reproduction in
   `hono-round30-types.md`): a generic callable's slot is `Fn(.., &T)`; substituting `T := tuple`
   must not turn it by-value. Carry the declaration's parameter-passing mode with the
   instantiated `FunctionType` (or look it up from the declaration) rather than recomputing it
   from the substituted type.
4. **Manifest correctness** (two bugs found by both round-30 agents): `[sources] exclude` globs
   are matched against the process cwd instead of the manifest directory
   (`DependencyCollector::excluded_target` or its caller), and `smelt build` writes `.d.ts`/`.pyi`
   output into the source tree so a second build lowers its own output. Fix both in
   `smelt-transpiler`; add a test that runs a manifest from a different cwd and one that builds
   twice and gets identical output. Cargo-light; do it last.

## Agent F — emitter coercions and MIR moves (~40 errors)

1. **A generated union value dispatched through the erased `match`** (5 in `context.rs`:
   `SmeltUnion1307` BodyInit scrutinee matched with `SmeltUnknown::String(..)` arms; 4 in
   `body.rs`: `&mut SmeltUnion778` receiver matched with `SmeltUnknown::Object(map)` for a
   property insert). Rule: a value whose static type is a generated union is dispatched through
   its own enum arms and each arm takes the concrete rule (BodyInit arm → the typed `SmeltBody`
   constructor; object arm → the typed field write); the `SmeltUnknown` runtime-shape `match`
   is only for values that are statically erased. Fixture: a `string | Uint8Array | null` body
   init and a `{a: number} | {b: string}` receiver written through a union-typed local.
2. **Typed promise continuations and Response overloads** (E0271, 4 `Result<SmeltResponse>` vs
   `Result<SmeltUnknown>`, 2 `SmeltUnion66` vs `SmeltFuture`, `Option<ResponseInit>` vs
   `Option<SmeltResponse>`, `Option<..>` vs `SmeltResponse`): a `.then` callback on a
   `Promise<Response>` returns the callback's own type, not `unknown`, and a parameter typed
   `ResponseInit | Response` (the WHATWG overload Hono's `c.body(data, init)` uses) is a union,
   not `ResponseInit`. Diagnose each against the source line before fixing; name the rule.
3. **MIR `Operand::Move` of a local read again later** (5 E0382 in `html.rs`, `str` moved at
   `:357` and used at `:362`): the move/copy decision for an erased local must respect a later
   read in the same block; fix the liveness rule in `smelt-mir` (not by cloning at the emitter).
   Fixture: an `unknown` parameter assigned to two locals in sequence in one block.
4. **Remaining singletons**: `Rc` vs `Rc` (3), `f64` vs `SmeltUnknown` (2), `SmeltTypedArray` /
   `SmeltRequest` vs `SmeltUnknown` (2), `buffer.rs` `buffer_to_string` returning the
   `SmeltArrayBuffer` where `String` is declared (a `TextDecoder`/`String.fromCharCode` shape —
   check the source), E0425 `dispatch` not in scope in `compose.rs` (a nested function hoisted
   out of the closure that defines it) and `__smelt_fn_value_627`, E0609 `router` on `Hono`.
   Each gets a named rule or a written diagnosis; none gets an erasure.

Notes: `blocker-logs/hono-round31-generics.md` (E) and `blocker-logs/hono-round31-emitter.md` (F),
allowlisted.
