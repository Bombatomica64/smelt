# H54 — a callback's return type erases when its body reads a field of the class being lowered

The router slice's stop after H51 (round 24). **Diagnosed then; ruled and FIXED
in round 25**, though the cause turned out to be one layer up from where this
note first put it — the correction is at the end.

## How it reports

```
Error: EmitError { message: "list unshift item must match the list element type" }
```

No file, no function: the list-mutation emitter's blockers do not use the
`Mir::file_paths` / `current_function_site` plumbing, so the whole slice build
says only that some `unshift` disagreed with its list. (The same complaint as
H48's closing note about the tuple-index blocker. Teaching these emitter
blockers to name their function is a small, independently useful change.)

The site is `src/router/reg-exp-router/node.ts:158`:

```ts
const strList = childKeys
  .map((k) => { ... typeof c.#varIndex === 'number' ? `(${k})@${c.#varIndex}` : ... })
  .filter(Boolean)

if (typeof this.#index === 'number' && this.#index !== -1) {
  strList.unshift(`#${this.#index}`)
}
```

`strList` should be `string[]`. It is `List<Unknown>`, so unshifting a `String`
mismatches.

## Cause, reduced to 12 lines

```ts
class Node {
  children: Record<string, Node> = {};
  varIndex?: number;

  a(keys: string[]): string[] {
    return keys.map((k) => {
      const c = this.children[k];
      return typeof c.varIndex === 'number' ? 'n' : k;
    });
  }
}
```

The callback's HIR type comes out `fn(String, Int, List<String>) -> Unknown`,
although BOTH ternary arms are strings. Narrowed by bisection:

| callback body | callback return |
| --- | --- |
| `typeof this.varIndex === 'number' ? 'n' : k` | `String` |
| `` typeof this.#varIndex === 'number' ? `(${k})@${this.#varIndex}` : k `` | `String` |
| `const c = this.children[k]; typeof c.varIndex === 'number' ? 'n' : k` | **`Unknown`** |

So it is not privacy (a public field behaves the same) and not the template
literal. What breaks it is the receiver being ANOTHER INSTANCE of the class
currently being lowered, reached through a record index: `this.children[k]` is
`Node` while `Node`'s item does not exist yet, so the field read resolves
through the in-progress path, which knows the class's METHODS
(`in_progress_class_method_member_type`) but not its FIELDS — and answers
`Unknown`.

The erased field would be harmless on its own; it is only the CONDITION of a
ternary whose arms are both `String`. The bug worth fixing is therefore two
layers deep:

1. the in-progress class metadata has no field lookup to match its method
   lookup, so a self-referential field read erases; and
2. a conditional whose condition is erased should still take its type from its
   ARMS — the union of two `String` arms is `String`, whatever the condition is.

(2) alone would fix the slice; (1) is the more precise fix and would also give
the field read its real type. Both are general rules, not Hono-shaped.

## Why it matters beyond the slice

Self-referential class shapes are ordinary (any tree or linked node: Hono has
two of them), and the failure is silent until something type-checks the result
— here an `unshift`, which is why it surfaced as an emitter blocker rather than
a frontend diagnostic. Anything that merely stores or joins the erased list
would have compiled and quietly carried `SmeltUnknown` values.

## Correction and fix (round 25)

The bisection above was right about the symptom and wrong about the cause, and
the correction is worth keeping because it changed what got fixed.

This note blamed the in-progress class metadata (a field lookup missing where a
method lookup exists) and the conditional's condition. Neither was it. Rerunning
the bisection one step further:

| callback body | callback return |
| --- | --- |
| `const c = 1; return k;` | `String` |
| `return this.label;` | **`Unknown`** |
| `const c = this; return k;` | **`Unknown`** |
| `const c = this.children[k]; return k;` | **`Unknown`** |

ANY block-bodied callback that mentions `this` erased — including one returning
a plain `string` field, and one that never reads a field at all. So it was not
field resolution: a field read resolves correctly (`let v: Option<f64>` for
`c.varIndex`, verified by running it). `this` merely forces the callback out of
the compact callback IR, which has no `this`, into the closure-body FALLBACK —
and the fallback took its return type from the CALLER's guess. For `map` that
guess is `unknown`, because the mapped element type is exactly what the callback
is supposed to answer.

The rule that landed: a block-bodied callback's return type is the join of its
own `return`s (the same join a ternary's arms use), not the caller's fallback.
An expression-bodied arrow already did this; the block form kept the fallback.

**And one condition, which the corpus taught rather than review.** The first
version inferred from the explicit returns alone and took es-toolkit from
1055/4 to 1047/12: a body that can fall off its end answers `undefined` on that
path, and for a customizer protocol that path IS the contract — `mergeWith`'s
customizer returns a value to override and falls through to mean "not handled".
So the inference only applies when the body cannot fall through, tested with
`statement_terminates`, the same conservative check the switch lowering uses.
A `no` from it keeps the caller's fallback, which is the pre-existing behaviour.

Measured: es-toolkit back to 1055/4 with avoidable erasure **32438 -> 32288
(-150)**, baseline re-snapshotted in the same commit as the policy requires;
remeda advisory -12; examples invariant 0. Fixture
`75_callback_block_return_type` covers five fallback shapes (loop, compound
assignment, `try`/`catch`, `switch`, numeric) plus the two shapes that already
worked, and every mapped list is pushed through an operation that only
type-checks at the concrete element type — a `List<Unknown>` prints the same
text, so the types are the assertion.

The slice is past this and stops on the next family, now named precisely by the
round's diagnostic work: `optional record field '#pattern' is unknown on Node_1
(in 'insert' at src/router/trie-router/node.ts:766..1681)`.
