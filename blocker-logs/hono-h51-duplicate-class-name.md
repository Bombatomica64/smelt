# H51 — two modules exporting a same-named class collide

Hono blocker 1 of the post-import-closure list, **diagnosed and not fixed**: the
fix is a naming decision that shows up in the generated Rust API, so it wants a
ruling before it is built.

## The blocker as reported

```
src/router/trie-router/node.ts    unknown class method `#push_handler_sets`
src/router/trie-router/router.ts  unknown class method `search`
```

Both names exist, are declared in the obvious place, and lower fine when
`trie-router` is transpiled on its own. They only go missing in the router slice
and in the whole crate.

## Cause, reproduced in 20 lines

Two modules each export a class named `Node`:

```
src/router/reg-exp-router/node.ts:47:  export class Node {          // no `search`, no `#pushHandlerSets`
src/router/trie-router/node.ts:20:    export class Node<T> {        // has both
```

A crate holding both keeps ONE class under the name, so every method lookup for
the loser reports "unknown class method". Minimal repro (a self-contained
project, not Hono):

```ts
// src/alpha.ts
export class Node {
  #items: string[] = [];
  push(value: string): void { this.#items.push(value); }
  search(prefix: string): string[] { return this.#items.filter((i) => i.startsWith(prefix)); }
}
// src/beta.ts
export class Node {
  index = 0;
  bump(): number { this.index += 1; return this.index; }
}
// src/main.ts
import { Node as AlphaNode } from './alpha';
import { Node as BetaNode } from './beta';
const alpha = new AlphaNode(); alpha.push('abc'); console.log(alpha.search('a').join(','));
const beta = new BetaNode(); console.log(beta.bump());
```

```
Error: unknown class method `bump`
```

The import ALIASES are distinct (`AlphaNode`, `BetaNode`) and it still
collides, so this is not an import-scope bug: class identity itself is the bare
source name. `Type::Class { name: Symbol, args }` carries only that name, and a
crate-level lookup by name cannot tell the two apart. `ClassRegistry` is
per-module (its docs say so), which is why single-module lowering is fine.

## Why it appeared now

The slice and the whole crate transpiled with these numbers for the whole
campaign because the manifest import scanner stopped at the first multi-line
`import { ... }` without a semicolon, and Hono is written without semicolons —
so `reg-exp-router/node.ts` and its `Node` were never in the crate. With the
scanner fixed (integration commit `461d8a81`) both `Node` classes are present
and the collision is live. Consequences already visible: the router slice does
NOT transpile at all right now, so its "7 errors" figure is superseded and
currently unmeasurable, and the whole-crate probe reports these two occurrences.

## The fix, and the decision it needs

Classes need crate-unique identity, the way MODULE bodies already get it:
`manifest_module_names` in `crates/smelt-transpiler/src/lowering.rs` gives
same-stem modules a stable ordinal suffix and keeps the last one unsuffixed (so
an entry `main` still emits `fn main()`). The same shape applies here — the
first colliding `Node` becomes `Node_1`, the last keeps `Node` — but for classes
it lands in three places at once:

1. class declaration: the symbol interned for the class, when another module in
   the crate declares the same name;
2. import resolution: an imported class name must resolve to the exporting
   module's symbol, not to the bare name;
3. emitted Rust: the struct/impl name follows the symbol, so a generated API
   gains `Node_1`.

(3) is why this is a ruling and not just a patch: it changes generated type
names for any project with a duplicated class name, and the alternative —
qualifying by module path (`trie_router_node_Node`) — reads better in a
stack trace but churns more names. There is also a middle option: keep the bare
name in `Type::Class` and add a module discriminant beside it, which leaves
emitted names alone but touches every construction and comparison of
`Type::Class`.

Recommendation: the ordinal scheme, for symmetry with module naming and because
it leaves every non-colliding project's output byte-identical.

## What is NOT the cause

Ruled out by the repro rather than by inspection: private-name mangling
(`#pushHandlerSets` -> `#push_handler_sets` folds identically at declaration and
use — a same-named private method resolves fine in a single module), optional
trailing parameters, generic class parameters, and import aliasing.
