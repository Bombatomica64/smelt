# Hono phase 3, round 1 (orchestrator brief)

Base: `origin/lorenzo/great-rubin-vy0943` (main + round 34; merge it first). Read
`hono-phase3-brief.md` in full, including its "Corrections from the pre-round survey" section,
and `hono-phase3-survey.md`. Round 34's last item (a MIR closure-capture fix, Agent M) runs
concurrently; the non-test crate is expected to reach 0 errors when it merges. This round does NOT
wait for that: everything below is frontend/manifest work.

| # | item | owner |
| --- | --- | --- |
| 1 | The test glob: `.github/compat/hono/Smelt.toml` `test-prefix = ["**/*.test.ts"]`. Plus a manifest-load diagnostic in `discover_test_paths` (`smelt-transpiler/src/lowering.rs`): a `test-prefix` glob that matches zero files under every root is a **warning** printed by `smelt build`/`probe` naming the glob and the roots. Unit test for it. | **Agent N** |
| 2 | Exclusions with a one-line reason each in the overlay: `src/middleware/cache/index.test.ts`, `src/adapter/service-worker/handler.test.ts`, `src/middleware/jwk/index.test.ts` (`vi.stubGlobal` / spy on a real global). Decide `vi.stubEnv` (3 files) and `vi.spyOn(console.*)`/`vi.spyOn(Date,'now')` (4 files): model them if the spy model already covers host-module members generally; otherwise exclude with a reason and count them in the note. | **Agent N** |
| 3 | Matcher model (`smelt-frontend-ts/src/lowering/testing/matchers.rs`, closed `TestMatcher` enum): add `toBeTruthy`, `toBeFalsy`, `toBeDefined`, `toMatch` (string and RegExp argument), `toThrowError` (= `toThrow`), `toMatchObject`, `toHaveBeenCalled`, `toHaveBeenCalledOnce`, `toHaveBeenNthCalledWith`, `toBeLessThan`, `toBeLessThanOrEqual`, `toBeGreaterThan`, `toBeGreaterThanOrEqual`, `toBeTypeOf`, and the jest aliases `toBeCalled`/`toBeCalledWith`/`toBeCalledTimes` as table entries onto the canonical arms. `toBeFunction`/`instanceOf` on `expect` are not vitest core: check whether Hono adds them via `expect.extend` in a setup file; if so, that is an `expect.extend` model question — report, do not special-case. Each matcher gets the runtime assertion in `smelt-stdlib`'s test support plus a frontend snapshot test; extend the existing vitest runtime tier. | **Agent N** |
| 4 | Bring the test closure in: `smelt probe`/`smelt build` on Hono with tests enabled. Metric: files-with-blockers → 0 across the 91 in-scope test files (minus item 2 exclusions), and the `#[test]` count in `dist-smelt`. Blockers that are general language features are numbered families with fixtures (take **116**, **117**, …); vitest members are model extensions. Then run `cargo check --tests` on `dist-smelt` ONCE and record the per-code error table in the note — do not start fixing test-build errors, that is round 2's table. | **Agent N** |

Rules: `implementer-brief.md` (setup step: merge `origin/lorenzo/great-rubin-vy0943`, not the retired
integration branch). Gates unchanged; es-toolkit baseline is now 31431. Note:
`blocker-logs/hono-phase3-round1.md` (allowlisted). Also update the phase-3 line in `hono-current.md`.
