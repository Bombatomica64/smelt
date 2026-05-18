# Remeda Generated Rust Errors

## Additional Web Server Probes (2026-05-16)

Generated from:

```sh
cargo run -q -- build --manifest-path /tmp/Smelt.strapi.toml
cargo run -q -- build --manifest-path /tmp/Smelt.strapi-core.toml
```

Additional logs:

- `blocker-logs/strapi-smelt-build.log`
- `blocker-logs/strapi-core-smelt-build.log`

### Strapi (`third_party/strapi`)

- Narrow entry (`examples/kitchensink-ts/src/index.ts`) builds cleanly with Smelt.
- Core entry (`packages/core/core/src/index.ts`) fails in frontend TS lowering at
  `packages/core/core/src/configuration/urls.ts` with:
  - `smelt::unsupported-ts`: `Map.get requires exactly one key argument`
  - `smelt::unsupported-ts`: `string prefix/suffix methods require string receiver and argument`

### Nest (`third_party/nest`)

- Probe blocked due to invalid subtree contents: `third_party/nest` currently contains a copy of
  this `smelt` repository tree (for example `AGENTS.md`, `Cargo.toml`, `crates/`, and
  `blocker-logs/` at its top level) instead of NestJS sources.
- Until the subtree is corrected to actual `nestjs/nest` content, any Smelt compile result would
  be invalid as benchmark signal.

Generated from:

```sh
cargo run -q -- build --manifest-path third_party/remeda/Smelt.toml
cargo check --manifest-path third_party/remeda/dist-smelt/Cargo.toml
```

Full compiler output:

- `blocker-logs/remeda-cargo-check.log`
- `blocker-logs/remeda-errors-extract.txt`
- `blocker-logs/remeda-error-codes.txt`

## Error Code Counts

| Count | Code | Main shape |
| ---: | --- | --- |
| 27 | E0308 | mismatched generated Rust types |
| 30 | E0277 | missing trait impl / invalid comparison / `?` in closure |
| 29 | E0618 | attempting to call non-function erased values |
| 17 | E0282 | inference needs explicit types |
| 14 | E0609 | field access on erased `SmeltUnknown` |
| 4 | E0689 | ambiguous numeric type |
| 4 | E0369 | invalid binary operator operands |
| 2 | E0624 | private associated function |
| 2 | E0594 | cannot assign through captured immutable binding |
| 2 | E0525 | closure only implements `FnMut` where `Fn` is required |
| 2 | E0061 | wrong argument count |
| 2 | E0057 | wrong closure call argument count |
| 1 | E0790 | associated function called on trait without impl type |
| 1 | E0606 | invalid cast |
| 1 | E0605 | invalid cast |
| 1 | E0596 | mutable borrow required |

## Current Focus

The largest family is `E0308`. The first fixed sub-shape was erased return
types rendered as `SmeltUnknown` while the emitted operand remained a concrete
Rust container, especially `HashMap<String, ...>`, `Vec<...>`, or `()`.
That reduced `E0308` from 787 to 688. A follow-up pass added generic
`Option<T> -> T` branch coercion and list item assignment coercion, reducing
`E0308` to 635.

The current pass added destination-aware numeric widening, call-result coercion,
TypeParam-to-`SmeltUnknown` wrapping, and destination-aware reassignment through
places. That reduced `E0308` from 635 to 336. The largest remaining repeated
shapes are concrete values still flowing into erased `SmeltUnknown`, erased
values flowing back into concrete vectors/strings, and statement-only blocks
where a numeric or unknown value is expected.

The control-flow pass replaced synthetic top-level `break`/`continue` markers
with labeled block exits. That removed `E0268` entirely. `E0308` briefly rose
to 339 because typed block-expression mismatches were no longer hidden behind
invalid loop-control errors.

The latest pass fixed the main repeated `E0308` source: static call emission
was reading callee parameter `LocalId`s from the caller's local table even
though MIR local IDs are function-scoped. That made generic helper calls like
`heapMaybeInsert<T>` adapt arguments to unrelated caller locals, producing many
`SmeltUnknown`/`f64` mismatches. Static calls now read parameter declarations
from the callee function, and generated list literals wrap elements when the
destination is `Vec<SmeltUnknown>`. `E0308` is now 150.

The current pass fixed dictionary/string-key adaptation for erased key values
such as Remeda's `PropertyKey`, including callback-expression dynamic indexes,
and widened nullish equality lowering for erased class-shaped values. That
removed the repeated `HashMap<String, _>::get(&SmeltUnknown)` trait failures
and lowered `E0308` to 85 and `E0277` to 82. The largest remaining `E0308`
shape is now statement/control-flow blocks with empty branches used where a
typed expression is expected.

The latest stdlib-property pass lowered `.length` and `.size` on erased
receivers through `Len`, added runtime `SmeltUnknown` length extraction for
strings/arrays/objects, and routed object destructuring of `length`/`size`
through the same path. That reduced `E0609` from 140 to 15 without increasing
moved-value errors. The remaining `E0609` sites are true erased structural
field reads such as lazy-result fields, regex groups, iterator result fields,
and optional regex field access.

The callback ownership pass eliminated `E0382` by emitting function parameters
as borrowed `&mut dyn FnMut(...)` values and forwarding callback arguments with
mutable reborrows or temporary borrowed adapters instead of `Box::new(...)`.
The `purry` special case now uses the same borrowed function-parameter adapter,
and the synthetic lazy helper returns a boxed closure again. This exposes the
next callback-shape blockers: boxed callback containers still try to clone
non-cloneable closures, and several generated closure calls now have arity
mismatches.

The rest-vector callback adapter now also recognizes erased generic and never
rest-item types that still render as `Vec<SmeltUnknown>`. That fixed most of
the `purry` adapter arity fallout: `E0057` dropped from 85 to 2 and `E0308`
dropped from 113 to 98. The `new Array<T>(length)` lowering now preserves its
explicit type argument instead of always constructing `List<Unknown>`, which
fixed the `new Array<T[]>(chunks)` result shape in `chunkImplementation` and
lowered `E0308` to 97. Remaining `E0308` sites are now mostly control-flow
expression blocks, erased optional/string comparisons, and lazy callback shape
mismatches rather than the rest-adapter cluster.

The current E0308 pass fixed three more repeated shapes: forward `then` blocks
that jump into loop/terminating successors are emitted inside the branch instead
of dropping the successor; all-rest tuple spread return types such as
`[...T1, ...T2]` lower to a list surface instead of an empty tuple/unit; and
`Object.fromEntries` over `PropertyKey` maps coerces keys to strings. Optional
`unknown_is Null` checks now emit `.is_none()` for `Option<T>`, and owned
function-shape adapters are boxed when returned. `E0308` is now 83.

The first E0599 pass fixed Rust emission for JavaScript numeric truncation
surfaces. Bit shifts and `number.toString(radix)` now truncate through an
explicit `as f64` cast instead of calling `.trunc()` on integer expressions.
That reduced `E0599` from 134 to 83 and also collapsed ambiguous numeric
`E0689` from 74 to 4. A follow-up callback stdlib pass taught callback
lowering and emission that `Set.has(...)` returns `bool` and emits Rust
`HashSet::contains`, reducing `E0599` to 82. The largest remaining E0599 shape
is boxed callback containers trying to clone non-cloneable `dyn FnMut` values.

The current E0599 pass eliminated the remaining E0599 compiler errors. It
added function-aware list callback emission so `some`/`every` over boxed
callbacks use `iter_mut()` instead of cloning callback boxes, skipped unused
callback parameter bindings in static/default callbacks, avoided cloning spread
function vectors by draining them, moved function-list `entries()` projections
through `into_iter()`, and routed optional/string stdlib surfaces that were
emitting invalid Rust methods. Float-backed `Set<number>` containment now uses
iterator equality instead of `HashSet::contains`, and float set add no longer
calls `insert`. `E0599` is now 0. The remaining float-set representation still
shows up under `E0277` because `HashSet<f64>` itself is not a valid Rust set
type.

The follow-up pass kept `E0599` at 0 and fixed two repeated generated-Rust
type mismatches. Equality between erased `SmeltUnknown` values and concrete
values now wraps the concrete side before comparison, so cases like
`unknown === "trailing"` emit comparable Rust values. Signed shift lowering now
uses the expression destination type for its final cast, so bigint shifts used
as `i64` no longer return `f64`. That reduced `E0308` from 116 to 88. The
runtime `IntoSmeltUnknown` bridge also now supports `Option<T>`, mapping `None`
to `SmeltUnknown::Null`; that reduced `E0277` from 92 to 83. The current
largest remaining blockers are statement/control-flow blocks used as typed
expressions, boxed callback/lazy-function shape mismatches, and concrete
container values that still need erased `SmeltUnknown` wrapping at call
boundaries.

The next pass fixed erased nullish coalescing and generalized map-to-unknown
conversion. Nullish coalescing over erased `unknown`/type-param/union values
now emits a `SmeltUnknown::Null` match, wraps into `Option<SmeltUnknown>` when
the MIR destination is optional, and casts back out when the destination is a
concrete type. `Option<T>` wrapping into `SmeltUnknown` now preserves `Some`
values instead of always becoming `Null`. `IntoSmeltUnknown` for hash maps now
accepts erased/property-key-like keys by converting keys through
`IntoSmeltUnknown` and then to JS-style string object keys. Current counts are
`E0308` 80 and `E0277` 77. The remaining largest shapes are callback/lazy
function type mismatches, expression blocks with statement-only branches, and
mixed numeric optional timer values.

The E0308-focused pass widened several coercion paths instead of special
casing Remeda call sites. Erased equality now treats union operands like
`SmeltUnknown` before comparing with concrete values. Dictionary index
assignment and dict-set runtime paths coerce keys and values to the receiver's
declared key/value types. Operand coercion now maps `Vec<T>` into `Vec<U>` when
the item types differ, wraps erased values before assigning to
`Option<erased>`, and treats erased source unions consistently. String
character splitting is destination-aware, so `string` to `unknown[]` maps each
character into `SmeltUnknown::String`. List and dict literals now wrap when the
destination is a type parameter or union-erased value, not only exact
`unknown`. `E0308` is now 64.

The next E0308 pass fixed a control-flow recognition gap and two callback
coercion gaps. Loop detection now follows internal join-block gotos, so
for-loop bodies with nested if/else joins emit a Rust `while` instead of a
tail-position `if` whose branches return `()`. Loop branch emission now also
tracks visited blocks to avoid recursively re-emitting the loop header. Closure
call rvalues are coerced through their function return type before assignment
to the destination, which fixes concrete callback results assigned into
optional locals. Callback-expression string concatenation now uses
`format!("{}{}", ...)` instead of raw Rust `String + rhs`, avoiding `&str`
mismatches for conditional string RHS expressions. `E0308` is now 48.

The first E0277 pass removed the biggest invalid trait-bound cluster. Function
types now synthesize real no-op callback defaults instead of emitting
`Default::default()` for `dyn FnMut` boxes, and unknown-to-function casts use
that same default callback path. `IntoSmeltUnknown` now supports two-element
tuples, which fixes tuple callback results routed through erased purry
adapters. `SmeltUnknown` now implements `PartialOrd` with deterministic tag
ordering, so generic `<`/`<=` callback comparisons compile. Numeric binary
emission coerces operands to the destination numeric type, including
float-to-int truncation for bigint-like integer destinations. Optional erased
values can now coerce through their inner value into concrete destinations,
fixing optional property-key lookups. Exact same-type coercions now return the
source unchanged so optional-chain outputs do not pick up redundant remapping.
`E0277` is now 30.

The current control-flow/callback pass eliminated the branch-scoped temporary
cluster. Common-join `if` branches that both end by assigning the same MIR local
now hoist that local before the Rust `if` and assign inside each branch, so
values like `_smelt_tmp_3` are available to the post-join return/conversion
code. This removed `E0425` entirely. The first hoist attempt exposed
function-typed branch locals rendered as `impl FnMut` variable bindings; those
now use concrete `Box<dyn FnMut...>` type text. Closure parameter rendering now
marks function-typed parameters as `mut`, so generated bodies can call boxed
`FnMut` arguments through `&mut *closure_arg_n`. That reduced `E0596` from 33
to 1. Current counts are `E0308` 45, `E0277` 30, `E0618` 29, `E0282` 17, and
`E0609` 14. The largest remaining repeated blocker is erased callable shape
loss: arrays or locals typed as `SmeltUnknown` are later called as functions,
producing the repeated `E0618` cluster.

The latest E0308 pass fixed two callback coercion regressions and one purry
argument gap. Exact same-type coercions no longer bypass function adaptation,
so returning a local closure for a `Box<dyn FnMut...>` return type emits
`Box::new(...)` instead of a bare closure. Function-shape adapters now coerce
their callback return values with the normal destination-aware conversion path,
so concrete containers returned through erased callback surfaces become
`SmeltUnknown`. The `purry` special-call branch now also coerces the optional
lazy argument to the callee's declared third parameter instead of forwarding raw
`SmeltUnknown` placeholders. Current counts are `E0308` 33, `E0277` 30,
`E0618` 29, `E0282` 17, and `E0609` 14.

The follow-up E0308 pass fixed three smaller but repeated coercion holes.
`None` constants assigned into concrete destinations now use the destination
default value, so uninitialized concrete vectors emit `Vec::new()` instead of
`()`. Function-shape adapters now coerce forwarded arguments from the target
callback parameter types back into the source callback's parameter types, which
removed several `SmeltUnknown`/`Vec<SmeltUnknown>` and `f64`/`Option<f64>`
adapter mismatches. Regex `match`/find lowering now uses the string-like
coercion path for erased haystacks instead of passing `&SmeltUnknown` to
`regex::Regex::find`. Current counts are `E0277` 30, `E0618` 29, `E0308` 27,
`E0282` 17, and `E0609` 14.
