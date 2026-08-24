// Fixture: fewer_param_maker_for_two_param_declaration
// Area: dispatch
// Guards: a 1-parameter arrow supplied where a 2-parameter maker is declared.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(make: (x: number, y: number) => T): T {
  return make(1, 2);
}

export function useStr(): string {
  return pick((x: number) => "a");
}
