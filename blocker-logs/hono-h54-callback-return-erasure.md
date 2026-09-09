# H54 — a callback's return type erases when its body reads a field of the class being lowered

The router slice's new stop, found the moment H51 let it get past the duplicate
`Node` classes (round 24). **Diagnosed, not fixed.**

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
