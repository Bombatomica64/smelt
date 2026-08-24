// Fixture: maker_return_pins_type_param_two_ways
// Area: adapter_substitution
// Guards: one callee pinned to `number` and to `string` purely through the callback return type.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(make: (x: number) => T): T {
  return make(1);
}

export function useNum(): number {
  return pick((x: number) => x + 1);
}

export function useStr(): string {
  return pick((x: number) => "a");
}
