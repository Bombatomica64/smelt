# Round 30 — whole-crate families (orchestrator brief)

Integration head: `claude/estoolkit-test-failures-4fuf9e` @ `c2f22267`. On that head the
Hono crate (ref `eebdf7be…` + `.github/compat/hono/.`) emits 33 modules and `cargo check` gives
**319 errors**. The round-29 "does not emit" report was an environment artifact: measured again
by the orchestrator from a clean `dist-smelt` with a freshly built full-feature binary.

| code | count | family (this round's owner) |
| --- | ---: | --- |
| E0107 | 136 | type-parameter defaults: `Context<E>` against a 3-parameter class (**Agent C**) |
| E0308 | 70 | `html.rs`: `buffer[0] += str` on `(string \| Promise<string>)[]` emits the erased number-coercion `match` against `SmeltUnion305` (**Agent D**) |
| E0382 | 8 | `html.rs`: the same union values moved then reused (`r`, `str`) (**Agent D**) |
| E0609 | 16 | `request.rs`: `req.raw.cache/credentials/integrity/keepalive/mode/redirect/referrer/referrerPolicy` missing on `SmeltRequest` (**Agent D**) |
| E0308 | 14 | expected `String`, found `SmeltHeaders` (**Agent D**, diagnose first) |
| E0308 | 10 | expected `SmeltUnknown`, found `SmeltRecord` (**Agent D**, diagnose first) |
| E0121 / E0063 | 10 / 5 | `HonoRequest<_, _>`, `Context_1<_, _, _>` written with `_` in item signatures and `_smelt_phantom` missing in initializers: a generic class referenced without type arguments in a signature position (**Agent C**) |
| E0308 / E0277 | 7+2 / 2 | `(SmeltUnknown, RouterRoute)` vs `SmeltUnknown`: tuple recovery through the callee-parameter coercion sites H42 left on the ambient scope (**Agent C**, after the defaults family) |
| E0277 | 3 | `?` inside a closure that does not return `Result` (`main.rs:11189/11367/11536`) (**Agent D**) |
| E0308 | 4 | expected `SmeltUnion288`, found `String` (**Agent D**) |
| other | ~10 | `Result` vs `Result`, `Rc` vs `Rc`, `Hono_1<…SmeltUnknown…>` vs `Hono_1<E,S,BasePath,CurrentPath>`, E0425 `__smelt_fn_value_627`, E0599, E0271 (record only, whoever meets them) |

Rules: `blocker-logs/implementer-brief.md` applies in full (fresh binary, clean corpora, fixture
numbers at commit time — next free is **90** — listed in `END_TO_END_EXAMPLES` in the same commit,
push after every commit, delete targets when done). General rules only: a fix must be stated as a
TypeScript-semantics rule, never as "what Hono spells".

Measurement: after each landed family, re-transpile Hono and report the per-code table against
319. The three accepted MIR outcomes of round 29 (H47 alternative, H48, the generic-base null
field) are NOT in this round; they are queued behind the crate compiling.

## Agent C — type lowering (frontend), 136 + 15 + 11 errors

1. **Type-parameter defaults.** A type reference that omits trailing type arguments takes the
   declaration's defaults (`class Context<E extends Env = any, P extends string = any,
   I extends Input = {}>` referenced as `Context<E>` means `Context<E, any, {}>`), and a
   reference that omits ALL arguments of a generic declaration means the defaults too where
   defaults exist. Resolve at the annotation-lowering site in `smelt-frontend-ts`
   (`lowering/ty/annotations.rs` and wherever instantiation expressions/heritage clauses lower
   type references), so HIR carries the full argument list and no later stage sees a
   short list. `any` defaults are the one genuine dynamic boundary here: document it at the
   lowering site. Fixture: a generic class with two defaulted parameters referenced with 1 and
   0 explicit arguments in a field type, a parameter type, a return type and an `extends`
   clause.
2. **Generic class referenced without arguments in a signature position** (E0121/E0063).
   Same rule as 1 when defaults exist; when they do NOT exist the reference is `tsc`-illegal
   except where inference supplies them (`new Foo(...)` with inferred arguments, `typeof`
   positions). Find which shape produces `HonoRequest<_, _>` in `clone_raw_request` and
   `Context_1<_, _, _>` in `compose` and lower it to the inferred or defaulted arguments; the
   `_smelt_phantom` E0063s fall out with it.
3. **Tuple recovery at callee parameters** (`(SmeltUnknown, RouterRoute)`): H42's note lists the
   five callee-parameter coercion sites still on the ambient render scope. Route them through
   `callee_parameter_render_scope` the way rung 7b already is, and make tuple types implement
   the recovery the rule needs (`SmeltFromUnknown` for tuples whose elements implement it) if
   that is what the remaining errors ask for after the scope fix. Report separately.

## Agent D — emitter and standards tier, 70 + 8 + 16 + 14 + 10 + 4 + 3 errors

1. **`+` / `+=` with a union operand.** TypeScript's rule: if either operand's type is or
   contains `string` (after the union's own arms are considered), `+` is string concatenation
   and the result is `string`; only when both operands are number-like is it addition. The
   emitter currently routes a union operand into the erased number-coercion `match`. Lower the
   union operand through its generated enum: a `string` arm concatenates directly, a non-string
   arm converts with the same string conversion `${}` templates use. Element writes back into
   the union list (`buffer[0] += str`) re-wrap in the union's string arm. Fixture: a
   `(string | Promise<string>)[]` buffer and a `(string | number)` accumulator with `+=`,
   verified against Node. The 8 E0382 moves in the same file are expected to be the same
   family (a union value read for the `match` and then reused); confirm, and fix the clone
   discipline generally if they are not.
2. **Request init fields.** `SmeltRequest` needs the remaining WHATWG `Request` members that
   `RequestInit` carries: `cache`, `credentials`, `integrity`, `keepalive`, `mode`, `redirect`,
   `referrer`, `referrerPolicy` (plus `signal`/`duplex` if not already there), as typed fields
   with the spec defaults, settable from the init record and readable as getters; the
   `standards-tier-plan.md` conventions apply. Fixture: construct a Request with every init
   field, read them back, `new Request(req, init)` override semantics.
3. **Diagnose then fix**: the 14 `String` vs `SmeltHeaders`, the 10 `SmeltUnknown` vs
   `SmeltRecord`, the 4 `SmeltUnion288` vs `String`. Name each family as a TypeScript rule in
   your note before fixing it; anything that turns out to be a single Hono spelling is recorded,
   not special-cased.
4. **`?` in a non-Result closure** (3): a fallible call terminator inside a closure whose
   generated return type is not `Result` must either make the closure fallible (when its
   contextual type allows a throwing callback) or route through the panic path the way the
   round-6 floor does. Follow `hono-fallible-ops.md`; do not silently drop the throw.

Notes: `blocker-logs/hono-round30-types.md` (C) and `blocker-logs/hono-round30-emitter.md` (D),
both allowlisted.
