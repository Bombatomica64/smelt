// A union of two or more present types and `undefined` is the optional of the
// present union, as an optional parameter `x?: A | B` is: narrowing past the
// nullish arm leaves the present union, and `typeof` picks its arms. A union
// whose absent value is `null` keeps `null` (and prints it) wherever it goes.
function w(x: string | number | undefined): string {
  if (x === undefined) {
    return "none";
  }
  if (typeof x === "string") {
    return "s:" + x;
  }
  return "n:" + (x + 1);
}

function len(x: string | number[] | null): number {
  if (x === null) {
    return -1;
  }
  return x.length;
}

function show(x: string | number | null): string {
  return String(x);
}

function pick(flag: number): boolean | string | undefined {
  if (flag > 1) {
    return "many";
  }
  return flag === 1 ? true : undefined;
}

console.log(w(undefined), w("a"), w(2));
console.log(len("abc"), len([1, 2]), len(null));
console.log(show(null), show(1), show("z"));
console.log(String(pick(0)), String(pick(1)), String(pick(2)));

// A `null` element stays `null` when the list reaches an `unknown[]`
// parameter: the element optional's absent value is the `null` written,
// whether the literal has one present type or several, annotated or not.
function lastIsNull(values: unknown[]): boolean {
  return values[values.length - 1] === null;
}
const single = [2, null];
const mixed: (string | number | null)[] = ["a", 2, null];
console.log(lastIsNull([2, null]), lastIsNull(single), lastIsNull(["a", 2, null]), lastIsNull(mixed));
console.log(lastIsNull([2, undefined]));
