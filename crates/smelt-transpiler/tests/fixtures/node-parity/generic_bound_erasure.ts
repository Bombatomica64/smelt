// A generic body whose only carrier uses erase a type parameter through its
// `IntoSmeltUnknown` bound keeps its generics, and stays erased where that
// would copy a container the caller can still observe.

function flattenAll(values: unknown[]): unknown[] {
  const out: unknown[] = [];
  for (const v of values) {
    if (Array.isArray(v)) {
      for (const inner of flattenAll(v)) out.push(inner);
    } else {
      out.push(v);
    }
  }
  return out;
}

// `arr.map(..)` is a fresh list handed once to a reading callee: erasing its
// `U` elements is unobservable, so `T` and `U` stay generic and the callback
// sees the caller's own array.
function flatMapAll<T, U>(
  arr: readonly T[],
  f: (item: T, index: number, array: readonly T[]) => U,
): unknown[] {
  return flattenAll(arr.map((item: T, index: number) => f(item, index, arr)));
}

const xs = [1, 2, 3];
let sameArray = true;
console.log(
  flatMapAll(xs, (x, i, a) => {
    if (a !== xs) sameArray = false;
    return [x, [x * 10 + i]];
  }),
);
console.log(sameArray);
console.log(flatMapAll(["a", "b"], (s) => s + s));

// The mapped list is handed to a callee that keeps it and then returned:
// erasing its elements would copy a list both sides still use, so this
// function stays erased and the callee's push is seen by the caller.
const kept: unknown[][] = [];
function keep(values: unknown[]): void {
  kept.push(values);
}
function grow(): void {
  const first = kept[0];
  first.push(99);
}
function mapAndKeep<T, U>(arr: readonly T[], f: (item: T) => U): U[] {
  const out = arr.map(f);
  keep(out);
  return out;
}
const source: unknown[] = [1, 2];
const mapped = mapAndKeep(source, (n) => n);
grow();
console.log(mapped.length, kept[0] === mapped);
