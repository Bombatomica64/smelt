# Design: Callable-Object Construction Dataflow (function-with-properties → callable-interface struct)

*(Fable planning pass, 2026-07-12. All code paths verified read-only.)*

## Verified current state
- **Interface side (done):** synthetic `__smelt_call: Type::Function(first call signature)` via `add_interface_call_signature_field` (frontend types_iface.rs:509-545); methods become function-typed fields (types_iface.rs:481-507). `DebouncedFunction<F>` is a struct with `__smelt_call`, `schedule`, `cancel`, `flush`.
- **HIR/MIR plumbing (partial):** `ExprKind::CallableObjectAssign { callable, props }` (smelt-hir expr/kinds.rs:546) and `Rvalue::CallableObjectAssign` (smelt-mir types.rs:1430; lower/expr.rs:1610-1625) exist — today produced ONLY by the `Object.assign(fn, {...})` stdlib path (frontend stdlib.rs:48-73), typed Unknown.
- **Emitter (half done):** `callable_object_assign_text` (call_runtime.rs:3037-3065) handles erased destinations only; for a concrete Class dest it silently returns just the callable and DROPS the props (line 3064). Read-back routing: call_runtime.rs:27-41, place.rs:189-199; erasure probe core.rs:4400-4416.
- **Bug mechanics in debounce.ts (:135,153-159):** `debounced.schedule = schedule` are member writes on a `Type::Function` local — no field, silently dropped; `return debounced` hits the coercion default fallback (coercion.rs ~600) producing `Default::default()` — inert struct.

## 1. Detection: frontend statement lowering (HIR), per-local forward collection
NOT a MIR pass — the writes are already lost by MIR time. Precedent: `ModuleBuilder.local_callbacks`. General rule (no name matching): "any function-typed local receiving static-member `=` writes whose value flows to a callable-interface-typed position."
- Add `callable_local_props: HashMap<LocalId, Vec<(Symbol, ExprId)>>` to ModuleBuilder (reset per body).
- In stmt/assignments.rs (next to the other `StaticMemberExpression` handlers): claim plain `=` writes whose object is an identifier bound to a `Type::Function(_)` local. Lower the RHS normally into a fresh compiler local (`Stmt::Let` — evaluation order and side effects preserved), record `(Symbol, local-read ExprId)`.

## 2. Field mapping
`Vec<(Symbol, ExprId)>` in source order, last write wins. At consumption, the target interface's field list (incl. `__smelt_call`) is known; each prop keeps its TYPED closure — coercion to the exact signature via the ordinary `value_at_type`/`rendered_function_shape_adapter_text` path. ZERO new SmeltUnknown; the erased branch stays only for the Object.assign dynamic boundary. The base callable maps to `__smelt_call`.

## 3. `return debounced` → struct literal
At any position where a function-typed local with recorded props coerces to a Class with a `__smelt_call` field (return values AND annotated lets): synthesize `CallableObjectAssign` typed at the interface Class. Emitter: add a typed-dest branch in `callable_object_assign_text` reusing `record_literal_text_for_dest` (call_runtime.rs:52-98 — field iteration, per-field value_at_type, Optional→default, `_smelt_phantom` for generics). Uncovered non-Optional fields = hard EmitError (better a build blocker than a silent Default). The coercion.rs ~600 fallback stays but stops firing.

## 4. Mutation semantics: value struct is correct
Keep by-value struct. `debounced.cancel()` works because stored closures capture shared state (timeoutId, pendingArgs) through the (Rc/RefCell-backed) closure environment; the struct is a bundle of Rc'd callables, clones share behavior. Reference semantics only matter for post-construction property REASSIGNMENT observed through an alias — debounce/throttle never do this. No Rc<RefCell> flavor for this feature.

## 5. Edge cases / documented punts
- Write after first escape: bail with `unsupported("property writes onto a callable local after it escapes are not lowered yet")`; flag flips on first non-write, non-self-call read.
- Conditional writes: claim only same-block straight-line writes; conditionals fall through + diagnostic (fields are non-Optional; would need Optional weakening — documented blocker).
- Self-calls of the local are fine (not an escape); passing as an argument is.
- Object.assign path: unchanged; typed-dest support comes free from the new emitter branch.
- Multiple call signatures: first only (existing rule).

## 6. Test plan
- Frontend: (a) callable-interface return with prop writes lowers to typed CallableObjectAssign; (b) write-after-escape/conditional-write produce documented errors; (c) plain function local untouched.
- Codegen (part_7_tests.rs, next to existing __smelt_call tests): struct literal with all fields, no `Default::default()`.
- Runtime proof: `smelt rust-test-report` focused on debounce.spec/throttle.spec (throttle wraps debounce, same trio of writes). Report to blocker-logs/callable-object-construction.md.
- smelt-unknown-report vs baseline: expected zero new avoidable-erasure.

## 7. Blast radius
Low and self-selecting: the new production fires only where output is already broken (dropped writes + Default::default()). Golden churn: debounce/throttle + anything sharing the pattern; everything else byte-identical (mtime-preservation intact). Emitter change is additive.
