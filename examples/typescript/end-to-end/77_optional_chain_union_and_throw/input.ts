// Three rules about an optional chain, all three of them families the Hono
// router slice hit:
//
// 1. a receiver whose static type is a generated UNION reaches the runtime
//    narrowing through its `IntoSmeltUnknown` boundary adapter, not by matching
//    `SmeltUnknown` arms against the union's own enum;
// 2. the erased element that narrowing produces is converted to the read's
//    declared RESULT type;
// 3. a method that can THROW propagates out of the chain, which means the
//    caller is a throwing function and the call carries its `?`.
type Slot = number | [string, number][];

function firstPair(slot: Slot): [string, number] | undefined {
  const pairs = slot?.[0];
  if (typeof pairs === 'number' || pairs === undefined) {
    return undefined;
  }
  return pairs;
}

const fromNumber = firstPair(7);
console.log('number arm: ' + (fromNumber === undefined ? 'none' : fromNumber[0]));
const pairs: [string, number][] = [['a', 1], ['b', 2]];
const fromPairs = firstPair(pairs);
console.log('tuple arm: ' + (fromPairs === undefined ? 'none' : fromPairs[0] + '=' + String(fromPairs[1])));

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
// function and the call inside the chain carries its `?` — a closure cannot,
// which is what made the optional chain answer `Option<Result<..>>`.
function fill(registry: Registry | undefined, key: string): string {
  registry?.insert(key, true);
  return registry === undefined ? 'no registry' : registry.has(key);
}

console.log('no receiver: ' + fill(undefined, 'a'));
console.log('receiver: ' + fill(new Registry(), 'a'));
