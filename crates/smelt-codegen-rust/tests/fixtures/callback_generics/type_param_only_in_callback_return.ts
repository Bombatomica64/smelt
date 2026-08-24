// Fixture: type_param_only_in_callback_return
// Area: callback_shape
// Guards: `T` appears only in the callback's return type.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(n: number, cb: (v: number) => T): T { return cb(n); }
export function use1(): number { return pick(1, (v: number) => v + 1); }
