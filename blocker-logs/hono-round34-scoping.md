# Round 34 — Agent L: synthesized-name scope and base-class resolution

Branch `worktree-agent-a04dfe69634dd264f`, base `main` @ `8d3b95fb` merged with
`origin/lorenzo/great-rubin-vy0943` (`b578552e`). Both assigned items landed.

## Hono, per code

Clean clone of `honojs/hono` @ `eebdf7be` + `.github/compat/hono/.`, fresh
full-feature `smelt`, repo-root absolute `--manifest-path`, `cargo check` on
`dist-smelt`:

| code | before | after | owner |
| --- | ---: | ---: | --- |
| E0425 `__smelt_fn_value_627` (`main.rs`) | 1 | **0** | L (item 2) |
| E0609 `router` on `Hono` (`main.rs`) | 1 | **0** | L (item 3) |
| E0425 `dispatch` (`compose.rs`) | 1 | 1 | K |
| **total** | **3** | **1** | |

The crate is one error away from compiling, and that error is Agent K's nested
function-declaration hoisting item. Nothing here touches it.

## Item 2 — `__smelt_fn_value_<key>` not in scope (E0425)

Erasing a named function to the dynamic carrier mints ONE per-item accessor
(`__smelt_fn_value_<key>`) so that every reference to that function shares one
erased value and compares equal under `===`. The name is minted at the erasure
site; the definition was flushed in `emit_source_with_free_function_router`
immediately after the free-function loop — **before** the class constructor and
method bodies, which that loop deliberately skips and the class loop below emits.
A function whose first erasure is inside a class body therefore referenced a name
the crate never defined.

Fix (`crates/smelt-codegen-rust/src/lib.rs`): the two accessor flushes
(`function_item_accessors`, `function_item_erased_fn_accessors`) moved to after
the class loop, i.e. after the last emission point that can mint one. The minting
scope and the definition scope are now one decision.

Regression test: `class_identity_runtime::a_function_value_first_erased_inside_a_class_body_is_defined`
(`cargo test -p smelt-codegen-rust --test class_identity_runtime -- --ignored`).
It covers both emission points — one function erased only from a method body, one
only from a constructor body — and asserts the identity the accessor exists for.

*Why a runtime tier and not an end-to-end fixture.* The fixture was written first
(`113_function_value_erased_in_a_method`) and Node 22 agreed with it, but the
examples corpus is the hard `SmeltUnknown` invariant: `avoidable erasure` must
stay 0, and the source `unknown` this shape needs classifies as avoidable (10
occurrences). Rather than widen `classify_line` for a whole spelling to admit one
fixture, the case moved to the tier, where `unknown` is not measured. The
avoidable-erasure invariant is therefore untouched.

## Item 3 — no field `router` on `Hono` (E0609)

Source shape (`.github/compat/hono` spells the alias on the EXPORT side, not the
import side as round 32 recorded): `hono-base.ts` declares `class Hono` and ends
with `export { Hono as HonoBase }`; `hono.ts` imports `{ HonoBase }` and declares
its own `class Hono extends HonoBase`. The two `Hono`s collide, so the base is
renamed `Hono_1`.

Three decisions had to stop keying on the source spelling.

1. **The export alias was never published.** `reexport_named_declaration` binds
   `exported -> item` only if `self.items` already holds the local, and that
   prepass visits `export { Hono as HonoBase }` before a non-exported
   `class Hono` has been predeclared. The rename was recorded and then dropped, so
   no module declared anything called `HonoBase`. `record_module_exports` now
   resolves the pending renames once the module's declarations exist, reading the
   class registry as well as the item map.
2. **`class_extends_clause` interned the LOCAL spelling.** An `extends` clause
   names a type, so it resolves like any other type reference
   (`resolve_type_reference_symbol`); and when that answers a symbol no class
   declares while the name is bound to a declaration under another spelling, the
   base recorded is that declaration's own symbol. Identity for a local
   unambiguous name, so nothing without a collision changes.
3. **Every base-chain walk keyed on the name.** A renamed class keeps its SOURCE
   spelling as its recorded name, so asking a by-name map about `Hono_1` answers
   with THIS module's `class Hono extends Hono_1` — the receiver itself, and the
   walk does not terminate (the stack-overflow trap `hono-round32-generics.md`
   §Item 3 predicted; it was hit and fixed, not worked around). Now symbol-keyed:
   * `ClassRegistry` carries `bases_by_symbol` beside `bases`, filled at
     `set_base` with the declaring class's own symbol, so it answers only for
     classes the module being lowered declared;
   * `in_progress_class_base_method`, `class_field_type`'s base fallback and
     `call_dispatch`'s method-field fallback read it;
   * `class_is_reproducible_base`, `reproducible_base_constructor_signature` and
     `inherited_base_fields` take the base SYMBOL and resolve the item with the
     new `class_item_by_symbol`. Before this the forwarded `super(...)` was
     silently DROPPED for an aliased base, so even with the fields present the
     subclass's `label` stayed empty.

### The follow-on this exposed: polymorphic `this` under flattened inheritance

With the base resolved, Hono's inherited methods appeared and turned the single
E0609 into 4 `E0308: expected Hono_1, found Hono`. A fluent base method is
declared with its own class and returns the receiver
(`route(..): Hono<..> { return this }`). At runtime that answers the SUBCLASS;
TypeScript spells the annotation nominally, and Smelt flattens inheritance into
separate structs, so the base spelling is a different Rust type.

Both ends now agree, under one predicate — *the declared return type is the
declaring class AND every `return` in the body answers the receiver*:

* the frontend (`resolve_method`'s base recursion) types a call through a
  subclass receiver at the subclass;
* the emitter renders the inherited copy's return type as `Self`.

A base method that returns a freshly built base instance (a `clone()`-style
factory) fails the body check and keeps the base type at both ends, which is what
keeps this a general rule rather than a fluent-API special case.

Regression test:
`hir_cli_cross_language_tests::build_inherits_through_a_base_class_exported_under_another_name`
— two modules, the export-alias shape, asserting the inherited field, the
forwarded `super(..)`, and a chained call through the inherited fluent method
against Node 22's output (`store:a/2` / `store:b`). It fails on any of the four
rules above: without (1)/(2) the subclass has no inherited field; without (3) the
frontend overflows its stack; without the follow-on the generated crate is E0308.

## Gates

| gate | result |
| --- | --- |
| Hono, clean clone, regenerated `dist-smelt` | **1 error** (E0425 `dispatch`, Agent K's), from 3 |
| examples invariant (`--fail-on-regression`) | avoidable erasure **0**, unchanged; exit 0 |
| es-toolkit ratchet | avoidable **31645**, baseline 31645, **delta +0** (no re-snapshot needed) |
| remeda generated `cargo test` | **1789 passed / 0 failed** |
| radash generated `cargo test` | **84 passed / 0 failed** |
| `cargo test -p smelt-frontend-ts --no-default-features` | 1093 passed / 0 failed |
| `cargo test -p smelt-codegen-rust` | 1079 passed / 0 failed (tiers `#[ignore]`d) |
| `cargo test --bin smelt` | 57 passed / 0 failed |
| `cargo test -p smelt-transpiler --test hir_cli_cross_language_tests` | 18 passed / 0 failed |
| `class_identity_runtime -- --ignored` | 5 passed / 0 failed |
| `cargo clippy --all-targets` | 0 errors; no new finding in any file touched |

`cargo clippy` needed `rustup component add --toolchain 1.96.1 clippy` in this
environment; it was not installed.

## SmeltUnknown delta

Net **zero**, measured in every corpus that measures it: examples avoidable 0
(unchanged), es-toolkit avoidable 31645 (unchanged), remeda untouched. Nothing in
this round introduces a `SmeltUnknown` conversion: item 2 moves an existing
accessor's DEFINITION, item 3 and its follow-on only replace name lookups with
symbol lookups and narrow a return type from a base struct to `Self`.

## Found, not fixed

* The examples corpus cannot host any fixture that spells `unknown`: the hard
  invariant classifies the resulting lines as avoidable erasure. That is why item
  2's end-to-end proof lives in a runtime tier. Worth deciding deliberately
  whether `classify_line` should treat a `let x: SmeltUnknown = ..` lowered from a
  source `unknown` annotation as a legitimate boundary — CLAUDE.md names source
  `unknown` as exactly that.
* The examples baseline's runtime-prelude count is still stale by 991 on main (the
  round-34 brief's housekeeping note). This round did not regenerate the corpus's
  goldens beyond the fixture it then removed, so it is left for whoever does.
