# TS7 parity, phase 2, round 2: URL, rest arrows, union narrowing (Hono)

Branch `feat/hono-url-model`. Measured on Hono v5 `0f5ad7b6` (the CI pin) with
the `.github/compat/hono` overlay: `smelt build`, then `cargo test
--no-fail-fast -- --test-threads=4` on `dist-smelt`, capped at
`MemoryMax=12G`, `CARGO_BUILD_JOBS=4`, debuginfo off.

| run | generated suite |
| --- | --- |
| round 1 (`main` at `dba17405`) | 395 passed / 59 failed of 454 |
| this round | **413 passed / 41 failed** of 454 |

No test that passed before fails now. All 18 `trailing-slash` tests pass.

## Rules added

Each rule is stated in TypeScript terms. Each has an end-to-end example whose
output is checked against Node, and whose Rust is checked for types.

1. **`URL` is a modeled class** (`lowering/stdlib/url.rs`, `url_prelude.rs`,
   `emitter/url.rs`). `new URL(x)` used to lower to its `href` string. A
   component read through a local answered `null`, and a component write
   (`url.pathname = ..`) was dropped silently.
   - `URL` is now `SmeltUrl`, a shared `Rc<RefCell<url::Url>>`. Component
     reads and writes go through `url::quirks`, the WHATWG accessor set.
   - `searchParams` is a live `SmeltUrlSearchParams` that owns the same cell.
     A params mutation runs the spec's update steps; a `search` or `href`
     write re-reads the list.
   - `new URL(..)` and the `href` setter throw a catchable `TypeError`. They
     are MIR call terminators (`BuiltinFn::UrlNew`, `UrlSetHref`), for the
     reason in `standards-throwing-rvalue.md`.
   - A `URL` passed to `new Request`, `fetch`, `new URL`, a template literal
     or `toString()` reads its `href`.
   - Example: `173_url_object`.
2. **Early-exit `instanceof` narrowing.** The false side of `x instanceof C`
   removes `C`'s arm from a union local (`inverse_guard_narrowing`). This is
   what `tsc` does. It declines when the remainder has an erased arm, because
   narrowing to `unknown` adds nothing and copies the value into an erased
   local.
3. **Assignment narrowing for union locals.** Assigning a value of one arm
   narrows later reads in the flow to that arm (`apply_assignment_observed_type`).
4. **`toString()` on a union** whose arms all have a typed string conversion
   (primitives, `URL`, `URLSearchParams`) is `String(x)`. The emitter renders
   it as one `match` over the `SmeltUnion` arms instead of erasing the union.
   - Examples for rules 2 to 4: `174_union_narrowing_exits_and_writes`.
5. **A rest arrow in a fixed-arity slot.** `(...args) => ..` stored in a
   fixed-arity callable slot receives every positional argument. The adapter
   packs the target's arguments into the rest list
   (`trailing_rest_element`). Before, it coerced `arg0` into the list and
   dropped the rest. Hono's `get newResponse()` lost the redirect status
   this way, so every 301 came back 302.
6. **A trailing list spread into a class method** binds the method's fixed
   parameters to the list's elements, as every other fixed-arity callee
   does. A type assertion around the list is erased at run time, so it
   decides neither the width nor the values.
7. **A null element is absent.** An erased `null`/`undefined` read into an
   optional slot is the absent case, not a value of the slot's type
   (`optional_value_map_text`, shared by the three `Option` to `Option`
   conversion sites).
8. **A contextual rest element type is a flat union.** A nested union cannot
   be a concrete enum, and it erased the whole rest list.
   - Example for rules 5 to 8: `175_rest_arrow_into_fixed_arity_slot`.

## Ratchets

| baseline | before | after |
| --- | ---: | ---: |
| examples SmeltUnknown avoidable (invariant) | 0 | 0 |
| es-toolkit SmeltUnknown avoidable (ratchet) | 22,372 | 22,341 |
| examples typedness | no program less typed | 3 new programs; the total gains 2 erased values from `173`'s `catch` binding (the documented `thrown.rs` boundary) |
| es-toolkit typedness | | erased values 10,191 to 10,175; erased reads and calls unchanged |

Two es-toolkit typedness records were updated by hand, with the user's
approval. In both, the erased count is unchanged and the generated Rust is
more typed. Only the typed share fell, because fewer MIR entries are counted
(the "per MIR entry" limit in `typedness-report.md`):

- `src/compat/object/pickBy.ts`, reads: concrete 5 to 3, erased 2 to 2. The
  old erased `keys[i].toString()` dispatch read `keys[i]` twice; the per-arm
  union match reads it once. Erased values fell from 27 to 23.
- `src/compat/string/truncate.ts`, values: concrete 75 to 74, erased 6 to 6.
  A `SmeltUnion` temp went away because `search` is now a plain `String`.

## Remaining Hono failures (41)

| file | failures | cause |
| --- | ---: | --- |
| utils/cookie | 14 | signed cookies need HMAC through `crypto.subtle` |
| mount | 13 | `matchedRoutes` reads `(c.req as unknown)[GET_MATCH_RESULT]`. That is a symbol-keyed getter `HonoRequest` declares, read behind a type-only `unknown`. The erased record has no such key, so `routePath(c)` is `''` |
| pretty-json | 7 | not investigated this round |
| cors | 3 | not investigated this round |
| timeout | 3 | not investigated this round |
| compose | 1 | not investigated this round |

## Found and not fixed

- `x: string | null` lowers to `Option<String>`, so `` `${x}` `` prints
  `undefined` for `null`. Same on `main`.
- A class method with a rest parameter (`count(...args: unknown[]) { return
  args.length }`) fails to lower: "unresolved identifier `args`".
- A list spread into a call on an overloaded callable-interface VALUE
  (`respond(...xs)`) still passes the list as one argument. That path picks
  the overload before it can distribute the list.
- Rest parameters are always lists. TypeScript types Hono's `(...args)` as
  the tuple `[string, (number | undefined)?, unknown?]`. A tuple-typed rest
  ABI would keep it fully typed.
