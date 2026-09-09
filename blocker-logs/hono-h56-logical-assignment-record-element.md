# H56 — logical assignment to a record element: two wrong values

Isolated in round 25 while reproducing the router slice's `insert`; **fixed in
round 26**, and the fix turned out to be much broader than the record element.
Both halves were silent wrong values, not blockers.

## What the measurements said

One generated crate, `Record` element targets, `Leaf` a class with a `tag`:

| # | source | expected | before |
| --- | --- | --- | --- |
| 1 | `a['k'] \|\|= new Leaf()` as a STATEMENT | `k` / `leaf` | correct |
| 2 | `const got = (b['k'] \|\|= new Leaf())` | `k` / `leaf` | **no keys** / `leaf` |
| 3 | `this.#children[key] \|\|= new Leaf()` as a statement | `present` | correct |
| 4 | `const child = (this.#children[key] \|\|= new Leaf())` | `leaf` / `present` | `leaf` / **`missing`** |
| 5 | `n['x'] ??= 2` on an absent number key | `2` | **`0`** |
| 6 | `m['y'] \|\|= 2` on an absent number key | `2` | correct |

Widening the probe past records showed the first half was not about records at
all — **every** assignment used as an expression dropped its store:

| source | expected | before |
| --- | --- | --- |
| `const got = (local = 5)` | `5` / `5` | `5` / **`0`** |
| `const got = (obj.field = 5)` | `5` / `5` | `5` / **`0`** |
| `const got = (rec['a'] = 5)` | `5` / key `a` | `5` / **no keys** |
| `const got = (local \|\|= 7)` | `7` / `7` | `7` / **`0`** |
| `const got = (obj.field \|\|= 7)` | `7` / `7` | `7` / **`0`** |
| `rec['a'] &&= 9` on an absent key | key stays absent | **key created** |

## The two causes, and the two rules

### An assignment is an expression whose effect is the store

`Expression::AssignmentExpression` lowered to the assignment's VALUE and
nothing else: the `Stmt::Assign` was only ever pushed by the statement, the
for-update and the mutable-global paths. `update_expression` (`x++`) already had
the correct shape — store into the current statement block, evaluate to a value
snapshotted before the store — so the fix is that shape for assignments, in
`assignment_expression_value`.

The stored value is ONE expression id referenced by both the store and the
result, so the right-hand side is evaluated once: `const held = (store['a'] =
new Leaf())` constructs one `Leaf`, and `held === store['a']`.

### A logical assignment stores only when its test says to

`t ||= v` is `t || (t = v)`. All three logical assignments lowered as the
unconditional `t = (test ? t : v)`, which is observationally equivalent for a
local or a field but wrong for a record element, where storing the value the
test rejected CREATES a key JavaScript leaves absent. `lower_logical_assignment`
now emits the branch — one rule for `||=`, `&&=` and `??=`, in statement and
expression position, over locals, fields and record elements. The right-hand
side is lowered inside the then block, so a logical assignment does not
evaluate it when the test keeps the current value.

The absent-key half is the read the OPERATOR tests: `SmeltRecord::get` already
answers `Option<V>`, and it is a read declared at type `V` that appends
`unwrap_or(<default>)`. So `Record<string, number>` answered `0` for a key that
was never written and `??=` saw a non-nullish value. The tested read is now
declared `Optional(V)`, which is what makes absence say `undefined`.
`logical_assignment_current_read` changes only that read; the plain read keeps
its declared type, so nothing else in the crate moves.

## What it fixed downstream

Hono's trie router writes exactly the shape in row 4:

```ts
const child = (curNode.#children[key] ||= new Node())
```

so `insert` built a child, handed it back and stored nothing: the whole trie
stayed empty even after H60/H61 made it compile.

## The es-toolkit ratchet: +50, and why

`avoidable-erasure` rose 32097 → 32147. None of it is a new erasure decision:
it is the text of stores and reads that were previously **missing**, over values
es-toolkit had already erased. The whole delta is six sites plus two:

| delta | site |
| ---: | --- |
| 12 | `assignValue.rs:20` (string index read arms) |
| 6 | `assignValue.rs:22`, `assignValue.rs:26` |
| 6 | `allKeyed.rs:10`, `allKeyed.rs:19`, `allKeyed.rs:29` |
| 6 | `cloneDeepWith_spec.rs:130` |
| 2 | `range.rs:52` |

`assignValue`/`allKeyed` assign through dynamic (`as any`, `PropertyKey`)
surfaces whose values are `SmeltUnknown` in the source contract, and the added
lines are the index read the store needs. Binding the value into a temporary
instead measured WORSE (+53), so the simpler shape is kept. The baseline is
re-snapshotted in the same commit with this accounting; a store that was
previously dropped is not an erasure regression to ratchet against.

## Guarded by

`examples/typescript/end-to-end/79_logical_assignment_store`, a runtime fixture
over all six measured shapes plus the plain-assignment and truthy-keep controls,
and `lowers_logical_or_assignment_as_a_conditional_store`, which asserts the
branch rather than the old conditional value.

Fixture 39 (`module_scope_reassignment`) exercises `||=`/`??=` at module scope:
its generated Rust changes shape and its stdout is byte-identical.

## Still open, found on the way

A plain read of an absent `Record<string, number>` key still answers `0` rather
than `undefined` (`String(rec['missing'])` prints `0`). Making every record read
`Optional(V)` is the honest rule — it is what `SmeltRecord::get` returns — but
it is a whole-corpus type change, so it is deliberately not in this commit.
