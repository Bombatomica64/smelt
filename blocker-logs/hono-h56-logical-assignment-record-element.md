# H56 — logical assignment to a record element: two wrong values

Isolated in round 25 while reproducing the router slice's `insert`. **Not
fixed**, numbered. Both halves are silent wrong values, not blockers.

## Measurements

One generated crate, six shapes, `Record` element targets. `Leaf` is a class
with a `tag` field.

| # | source | expected | actual |
| --- | --- | --- | --- |
| 1 | `a['k'] ||= new Leaf()` as a STATEMENT | `k` / `leaf` | `k` / `leaf` — correct |
| 2 | `const got = (b['k'] ||= new Leaf())` | `k` / `leaf` | **no keys** / `leaf` |
| 3 | `this.#children[key] ||= new Leaf()` as a statement | `present` | `present` — correct |
| 4 | `const child = (this.#children[key] ||= new Leaf())` | `leaf` / `present` | `leaf` / **`missing`** |
| 5 | `n['x'] ??= 2` on an absent number key | `2` | **`0`** |
| 6 | `m['y'] ||= 2` on an absent number key | `2` | `2` — correct |

Controls that are all correct: `record[key] = v`, `list[0] ||= 5`,
`obj.field ||= 7`, `plain ||= 9`.

## H56a — the assignment's VALUE is right but the store is dropped

Rows 2 and 4: when the logical assignment is used as an expression, the value
handed to the surrounding expression is the newly constructed one, but the
record is never written. As a statement (rows 1 and 3) the same source writes
correctly, so the read-modify-write exists; the value-producing path returns
the computed value and skips the store.

This is exactly what Hono's trie router writes:

```ts
const child = (curNode.#children[key] ||= new Node())
```

so `insert` builds a child, hands it back, and stores nothing: the trie stays
empty. It is why the two-`Node` reproduction printed `none/none` even after
H60/H61 made it compile.

## H56b — `??=` on an absent key sees the element type's default

Row 5 versus row 6: `??=` does not assign on an absent `Record<string,
number>` key, and the key reads back as `0`. `||=` on the same shape assigns.
Both follow from one cause: the computed read of an absent key yields the
element type's DEFAULT (`0`) rather than `undefined`, so the nullish test sees
a non-nullish value and skips the assignment while the falsy test does not. For
an object-valued record (row 1) the absent read is `undefined`, which is why
`||=` there is correct.

The spec answer is `undefined` for every absent key regardless of the declared
value type; `Record<string, number>` values are `number | undefined` on read.

## Why numbered rather than fixed here

Both halves live in assignment lowering and are independent of this round's
class-identity work (H51/H60/H61); H56a in particular changes the shape of
every logical-assignment expression value. Worth its own item, with the table
above as the fixture.
