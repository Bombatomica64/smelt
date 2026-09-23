# Round 34 — the last 3 (orchestrator brief)

Base: `main` @ `8bc1be60` (PRs #256/#257 merged, CI green). Working branch for the orchestrator:
`lorenzo/great-rubin-vy0943`. Hono (ref `eebdf7be…` + `.github/compat/hono/.`, clean clone, fresh
full-feature binary, repo-root absolute `--manifest-path`): 33 modules, **3 errors**.

| # | error | family | owner |
| --- | --- | --- | --- |
| 1 | E0425 `dispatch` not in scope (`compose.rs`) | nested function declaration hoisting. Fix exists: commit `a6b308a2` on `origin/worktree-agent-aefb17f85813045db` (cherry-pick it onto main; do NOT take `afaf178e`, it carries a `SMELT_NO_HOIST` debug switch that must not land). Held back because it raises the es-toolkit ratchet by 4: in `curry.rs` the self-recursive closure knot is `Rc<RefCell<SmeltErasedFunction>>` where the closure type is concrete. Type the knot at the closure's own `Rc<dyn Fn(..)>` type (the general rule: a recursive binding's cell carries the binding's declared/inferred type, never an erased function), so the ratchet stays ≤ 31645 (or falls: re-snapshot). | **Agent K** |
| 2 | E0425 `__smelt_fn_value_627` (`main.rs`) | a synthesized function-value name is minted at the definition scope and referenced from another; make the name's scope and the reference's scope one decision (`hono-round32-emitter.md` §"Also diagnosed") | **Agent L** |
| 3 | E0609 `router` on `Hono` (`main.rs`) | `import { Hono as HonoBase }; class Hono extends HonoBase`: `class_extends_clause` interns the LOCAL spelling of the base; every base-chain walk must key on the resolved symbol, and the in-progress registry must answer only for symbols the current module declares (stack-overflow trap: `hono-round32-generics.md` §Item 3; H61 family) | **Agent L** |

Rules: `blocker-logs/implementer-brief.md` in full, except: the integration branch there
(`claude/estoolkit-test-failures-4fuf9e`) is retired — merge `main` instead. Next free fixture
numbers: Agent K takes **112**, Agent L takes **113** and **114**. General TypeScript-semantics
rules only. Report the per-code table against 3 after each landed item. When the crate reaches
**0 errors**, also run `cargo build` on it and report whether it links; do not start on tests
(`hono-phase3-brief.md` has its own round).

Housekeeping (whoever regenerates the examples corpus first): the examples baseline's
runtime-prelude count is stale by 991 on main; re-snapshot it in that commit.

Notes: `blocker-logs/hono-round34-hoisting.md` (K) and `blocker-logs/hono-round34-scoping.md` (L),
allowlisted.
