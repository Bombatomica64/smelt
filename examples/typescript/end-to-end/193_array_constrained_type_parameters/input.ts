// A type parameter constrained to an array is generic over its ELEMENT.
//
// `T extends readonly unknown[]` with `T[number]` is how libraries spell "an
// array of anything, and its element" (remeda's `IterableContainer`). A hand
// port writes `fn drop_while<T>(data: Vec<T>, predicate: impl Fn(&T, ..)) ->
// Vec<T>`: the name `T` is a list of the parameter and `T[number]` is the
// parameter itself, so every value read out of `data` keeps the element type
// each call instantiates instead of erasing to `SmeltUnknown`.
//
// Covered: a function declaration called with inferred and with explicit
// (`number[]`, which names the whole list) type arguments, and a generic alias
// over the parameter, used inside a function and instantiated directly by a
// tuple, whose `[number]` is the union of its items.
//
// Every line below is diffed against Node.

type IterableContainer<T = unknown> = ReadonlyArray<T> | readonly [];

type Item<T extends IterableContainer> = T[number];

function dropWhile<T extends IterableContainer>(
  data: T,
  predicate: (item: T[number], index: number, data: T) => boolean,
): T[number][] {
  for (const [index, item] of data.entries()) {
    if (!predicate(item, index, data)) {
      return data.slice(index);
    }
  }
  return [];
}

function firstItem<T extends IterableContainer>(items: T): Item<T> | undefined {
  return items[0];
}

const numbers = dropWhile([1, 2, 10, 3, 4], (x) => x < 10);
console.log(numbers.join(","), numbers[0] + 1);

const words = dropWhile(["a", "bb", "ccc"], (word, index) => word.length + index < 3);
console.log(words.map((word) => word.toUpperCase()).join(" "));

const explicit = dropWhile<number[]>([5, 1, 7], (x) => x > 3);
console.log(explicit.length, explicit[1] * 2);

const tagged: Item<[string, boolean]> = "label";
console.log(tagged === "label");

const first = firstItem(["x", "y"]);
console.log(first === undefined ? "none" : first.repeat(2));
