// A method that can THROW inside an optional chain propagates out of the
// chain, which means the caller is a throwing function and the call carries
// its `?` — a closure cannot, which is what made the optional chain answer
// `Option<Result<..>>`.
//
// This fixture used to also pin the two UNION rules of the same Hono family:
// that a receiver whose static type is a generated union reaches the runtime
// narrowing through its `IntoSmeltUnknown` boundary adapter (not by matching
// `SmeltUnknown` arms against the union's own enum), and that the erased
// element narrowing produces is converted to the read's declared result type.
// Those two are an ERASED path by construction — an element read on a union
// whose arms are not all indexable has no concrete Rust type to carry — so
// keeping them here spent the zero-avoidable-erasure budget of the examples
// corpus on output whose whole subject is erasure. They live in the executing
// tier instead, where the assertion is the VALUE and not the spelling:
// `union_optional_element_read_narrows_at_runtime` in
// `crates/smelt-codegen-rust/tests/union_receiver_runtime.rs`. Fixtures 66 and
// 74 were split the same way.
class Registry {
  #seen: Record<string, boolean> = {};

  insert(key: string, value: boolean): void {
    if (key === '') {
      throw new Error('empty key');
    }
    this.#seen[key] = value;
  }

  has(key: string): string {
    return this.#seen[key] === undefined ? 'absent' : 'present';
  }
}

// `registry?.insert(..)` calls a method that can throw, so `fill` is a throwing
// function and the call inside the chain carries its `?`.
function fill(registry: Registry | undefined, key: string): string {
  registry?.insert(key, true);
  return registry === undefined ? 'no registry' : registry.has(key);
}

console.log('no receiver: ' + fill(undefined, 'a'));
console.log('receiver: ' + fill(new Registry(), 'a'));
