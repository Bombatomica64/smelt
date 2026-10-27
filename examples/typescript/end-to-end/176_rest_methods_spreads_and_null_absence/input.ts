// Rest parameters on methods, list spreads into every callee shape, and the
// `null` an optional holds when its type admits only `null`.

class Bag {
  items: number[] = [];

  count(...args: (string | number | boolean)[]): number {
    return args.length;
  }

  add(first: number, ...more: number[]): number {
    this.items.push(first, ...more);
    return this.items.length;
  }

  static join(sep: string, ...parts: string[]): string {
    return parts.join(sep);
  }

  get label(): Label {
    return (name: string, suffix?: string) => `${name}${suffix ?? "."}`;
  }
}

interface Label {
  (name: string, suffix?: string): string;
  (name: string): string;
}

const bag = new Bag();
const extra = [4, 5];
console.log(bag.count(), bag.count(1, "a", true), bag.add(1), bag.add(2, 3), bag.add(6, ...extra));
console.log(Bag.join("-", "a", "b", "c"), Bag.join("+"), ["x", "y"].join("/"));

const one: [string] = ["a"];
const named: string[] = ["b"];
const label: Label = bag.label;
console.log(label(...one), bag.label(...one), label(...(named as [string])), bag.label(...(named as [string])));

interface Entry {
  key: string;
  value: string | null;
}

function show(entry: Entry): string {
  return `${entry.key}=${entry.value}:${entry.value === undefined}:${entry.value === null}`;
}

function describe(value: string | null): string {
  return String(value) + "|" + value;
}

let pending: string | null = null;
console.log(show({ key: "a", value: null }), show({ key: "b", value: "x" }));
console.log(describe(null), describe("y"), pending, JSON.stringify({ key: "c", value: null } as Entry));
pending = "done";
console.log(pending, new Headers().get("missing"));
