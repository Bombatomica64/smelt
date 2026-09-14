// A closure that captures the enclosing method's receiver cannot bind it as
// `self` — that is a Rust keyword — so the capture takes a generated name and
// the closure body's uses are rewritten to it. `let self = self.clone();` is
// what the emitter produced before, which is E0424 and stopped the Hono router
// slice's `trie-router/node.ts` from compiling.
class Index {
  #known: Record<string, boolean> = {};

  add(key: string): void {
    this.#known[key] = true;
  }

  // The callback captures `this`, and it is passed where a callback value is
  // needed, so the receiver is cloned into the closure.
  keep(keys: string[]): string[] {
    return keys.filter((key) => this.#known[key] === true);
  }

  // Two captures in one closure: the receiver and a local.
  label(keys: string[], suffix: string): string[] {
    return keys.map((key) => (this.#known[key] === true ? key + suffix : key));
  }
}

const index = new Index();
index.add('a');
index.add('c');
console.log('kept: ' + index.keep(['a', 'b', 'c']).join(','));
console.log('labelled: ' + index.label(['a', 'b'], '!').join(','));
