// Fixture: generic_class_method_and_free_maker
// Area: dispatch
// Guards: a generic class method and a free generic function both taking a nullary maker.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export class Box<T> {
  run(make: () => T): T { return make(); }
}
export function feed<U>(make: () => U): U { return make(); }
export function top(): string { return feed(() => "a"); }
