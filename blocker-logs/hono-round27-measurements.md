# Round 27 measurements, and what the two leading families are

## The router slice: 6 errors, all H42

| n | code | shape |
| ---: | --- | --- |
| 2 | E0277 | a record built from an iterator whose items still carry `T` |
| 2 | E0308 | expected type parameter `T`, found `SmeltUnknown` |
| 2 | E0308 | `SmeltList<(T, …)>` vs `SmeltList<(SmeltUnknown, …)>` |

Unchanged from round 26's end: every remaining slice error is the deferred
`TypeParam` erasure family. The slice has stopped being a useful metric, which
is what the whole-crate table now replaces.

## The complete crate: 883 errors, reproduced exactly

33 generated files; the table matches the standards agent's report
(`blocker-logs/standards-hono-first-cargo-check.md`) error for error. The
leading groups:

| n | code | message |
| ---: | --- | --- |
| 180 | E0609 | no field `var_index` on type `Context` |
| 164 | E0107 | struct takes 0 generic arguments but 3 were supplied |
| 70 | E0308 | expected `SmeltUnion305`, found `SmeltUnknown` |
| 58 | E0107 | struct takes 0 generic arguments but 1 was supplied |
| 20 | E0609 | no field `finalized` on `Ref<'_, ContextInner>` |
| 20 | E0599 | no method `_new_response` on `Context` |

## The two leading families are ONE defect, and it is H51's family again

`src/router/reg-exp-router/node.ts` exports

```ts
export type Context = { varIndex: number }
```

a type ALIAS for a structural record, and `src/context.ts` exports

```ts
export class Context<E extends Env = any, P extends string = any, I extends Input = {}>
```

Two different type-level declarations of the name `Context`, in two modules.
HIR interns one symbol per name, so they collapse:

- the trie's `#context: Context = { varIndex: 0 }` is a record value whose type
  resolved to the CLASS, so `context.varIndex` reads a field the class does not
  have — **180 E0609s**;
- the emitted `struct Context` is the alias's record (no type parameters), while
  every reference from `context.ts` and `hono-base.ts` spells `Context<E, P, I>`
  — **164 + 58 E0107s**.

Together that is **402 of 883 errors (46%)** from one cause.

H51 gave a crate-ambiguous CLASS name an ordinal suffix, and H61 made the
declaring module's own binding win. Neither covers a type alias or an interface,
and all three kinds share one symbol space — so the rule to generalize is:

> a type name declared by more than one module is crate-unique across every
> type-level kind (class, interface, type alias), and a module's own
> declaration wins for its own spelling.

`manifest_class_renames` already computes exactly this for classes, keyed by
module path; `scan_declared_class_names` is the half that would grow to report
interfaces and aliases too, and `resolve_type_reference_symbol` already prefers
a scoped binding when one exists. The remaining question is what the RENAMED
alias/interface does to the emitted names of the structural types derived from
it, which is why this is its own round rather than a tail of this one.

## Everything else that round 27 measured

- radash 84/0; es-toolkit 1055/4 with avoidable erasure **32114** after the
  merge (−38 against my own snapshot); remeda 391 files, `cargo check` clean;
  examples invariant 0; workspace tests green with 86 end-to-end fixtures.
- `target-priv` was 11G (a full debuginfo build tree accumulated over the
  campaign); deleted at the round's commit boundary and rebuilt at **423M** with
  `CARGO_PROFILE_DEV_DEBUG=0`, which confirms the trimmed prefix is doing its
  job and the 11G was accumulation, not a missing flag.
