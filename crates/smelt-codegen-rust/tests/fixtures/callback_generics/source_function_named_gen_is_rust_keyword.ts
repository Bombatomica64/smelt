// Fixture: source_function_named_gen_is_rust_keyword
// Area: naming_collisions
// Guards: a source function named `gen`, a Rust 2024 reserved keyword; the same naming-collision
// class as a generated `F0` colliding with a source class named `F0`.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function gen<T>(cb: (v: T) => boolean, n: number): number {
  return n;
}
export function use1(): number {
  return gen((v: number) => v > 1, 3);
}
