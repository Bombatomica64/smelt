# H71 — a type name is crate-unique across KINDS, not per kind

Round 28, item 1. **Fixed.** 46% of the whole-crate `cargo check` errors, and the
largest family in `blocker-logs/hono-current.md`, were one wrongly resolved name.

## The stop it caused

```
437 × error[E0609]  no field `var_index` on ...
222 × error[E0107]  struct takes 3 generic arguments but 1 generic argument was supplied
```

Hono declares `Context` twice, in two different KINDS:

```
src/router/reg-exp-router/node.ts:9   export interface Context { varIndex: number }
src/context.ts:293                    export class Context<E extends Env = any, P extends string = any, I extends Input = {}>
```

H51/H61 gave a crate-ambiguous CLASS name a per-module rendering
(`manifest_class_renames` + `module_qualified_class_name`), and the scanner that
decides ambiguity (`scan_declared_class_names`) walked class declarations only.
So the interface and the class shared one symbol: reads of the interface's
`varIndex` landed on the class's struct (E0609) and the class's three type
parameters were supplied to the interface's none (E0107).

## The rule

A type name is crate-unique across **every** type-level kind — `class`,
`interface`, `type` alias, `enum` — because the generated Rust has ONE type
namespace: a class's struct, an interface's struct, a structural alias's struct
and an enum all land in it. Ambiguity is therefore decided by one scan over all
four kinds, and the declaring module wins for its own spelling whatever kind it
declared. The ordinal scheme is unchanged (H51): a name declared by exactly one
module is absent from the map and keeps its bare spelling, so no existing golden
moves; among several, the last in dependency order keeps the bare name and
earlier ones take `_1`, `_2`.

Renamed accordingly, because the machinery is no longer about classes:
`scan_declared_class_names` -> `scan_declared_type_names`,
`manifest_class_renames` -> `manifest_type_renames`, `HirCtx::class_renames` ->
`type_renames`, `module_qualified_class_name` -> `module_qualified_type_name`.
Interface, alias and enum declarations reach it through one new entry point,
`declared_type_name_symbol`, which consults the map only for a module-scope
declaration (a namespace-nested one already carries its namespace prefix and is
left alone).

## The second half: an import CYCLE

Fixing the scan alone took the whole crate from 883 to **352** errors but left
136 `E0107` of a NEW shape: `Context<E>` in `types.ts` and in the class's own
fields still resolved to the 0-parameter interface.

`resolve_type_reference_symbol`'s by-name lookups can only answer once the
imported module's ITEM exists, and in an import cycle it does not. Hono's
`types.ts` and `context.ts` import each other; `types.ts` lowers first, its
`Context<E>` found no item, fell through to the bare spelling — the interface's —
and the alias `NotFoundHandler<E>` baked that in. Every field typed through the
alias then read the interface.

The rename map has no such ordering problem: it is computed from a scan of every
source before any module lowers. So the question "which module was this name
imported FROM, and what is that module's rendering of it" is answerable at the
moment the reference is lowered, item or no item. That is
`imported_ambiguous_type_name` (with `module_type_rename` doing the path
matching, by manifest spelling first and by canonicalized path second). The
lookup is by the name the module EXPORTED, so `import { Node as TrieNode }` asks
about `Node`; `ImportScope` now records that mapping (`imported_names`) beside
the specifier it already recorded.

`resolve_type_reference_symbol` therefore answers in this order: a name this
module declares, then a name it imported, then the pre-existing crate-wide
lookups. Both new steps answer `None` for every unambiguous name, so a crate
without a collision resolves exactly as before — no golden moved.

## Measured

Whole-crate `cargo check` on `third_party/hono/dist-smelt`, committed overlay,
fresh clone at `eebdf7be`:

| code | before | after scan | after import fix |
| --- | ---: | ---: | ---: |
| E0609 (no field) | 437 | 21 | **17** |
| E0107 (generic argument count) | 222 | 114 | **136** |
| E0308 (mismatched types) | 135 | 145 | 138 |
| E0599 (no method) | 33 | 2 | 1 |
| E0277 (trait bound) | 29 | 32 | 7 |
| E0560 | 12 | 11 | 0 |
| E0382 | 8 | 8 | 8 |
| E0121 | 4 | 10 | 10 |
| E0425 | 2 | 2 | 2 |
| E0063 | 1 | 5 | 5 |
| E0271 | 0 | 1 | 1 |
| other | 1 | 1 | 1 |
| **total** | **884** | **352** | **326** |

E0609 is effectively gone (437 -> 17) and E0107's cause has CHANGED: every one
of the 136 that remain is now `struct takes 3 generic arguments but 1 generic
argument was supplied`, i.e. `Context<E>` written against
`class Context<E = any, P = any, I = {}>`. That is a different family — **type
parameter DEFAULTS**: a type reference that omits trailing type arguments must
take the declaration's defaults. It is now the largest single family in the
crate and is the natural next item.

## Guard

`build_runs_type_names_shared_across_kinds_and_modules` in
`crates/smelt-transpiler/tests/hir_cli_cross_language_tests.rs` — four modules,
build-and-run: a `type Slot = string | number` and an `interface Tag` against a
`class Slot<T>` and a `class Tag`, plus an import cycle (`readers.ts` type-imports
the class into `export type Held = Slot<string>`, `holder.ts` type-imports `Held`
back and calls `held.read()` through it). Verified to FAIL with the scanner
reverted to classes only: the generated crate does not compile.

## Found and NOT fixed

A **record-shaped type alias** loses its shape as a parameter type:

```ts
type Slot = { depth: number };
function bump(slot: Slot): number { return slot.depth; }
console.log(bump({ depth: 1 }));
```

```
Error: type table does not contain literal operand type Unknown
  at crates/smelt-codegen-rust/src/emitter/types.rs:849 (in `bump`)
```

Reproduced in a single module with no collision anywhere, so it is entirely
independent of this item — the rename map is empty for that project and every
code path added here is inert. It is unexercised because NO fixture in
`examples/typescript/end-to-end` declares an object-literal type alias (checked:
`grep -l '^type .* = {'` finds none); the `interface` spelling of the same shape
works. This is why the item-1 guard uses a union alias rather than a record one.

Also unfixed, and hit while writing the guard: passing a closure to a method
whose parameter type comes from a function-typed alias emits
`Rc<{closure}>` against a `&dyn Fn(..)` parameter (E0308). Both are worth their
own items.
