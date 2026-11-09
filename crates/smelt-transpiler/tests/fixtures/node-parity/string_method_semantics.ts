// String operations reached with concrete and erased receivers.

function firstA(names: string[]): string | undefined {
  return names.find((name: string) => name.startsWith("a"));
}
console.log(firstA(["b", "ab", "ac"]), firstA(["b"]) === undefined);

function label(value?: string): string {
  switch (value) {
    case "a":
      return "A";
    default:
      return "other";
  }
}
console.log(label("a"), label("b"), label());

type LocalizeFn<Value> = (value: Value, options?: { unit?: string }) => string;
const feminineUnits = ["second", "minute"];
const ordinalNumber: LocalizeFn<number> = (dirtyNumber, options) => {
  const number = Number(dirtyNumber);
  const unit = options?.unit;
  if (number === 0) return "0";
  let suffix;
  if (number === 1) {
    suffix = unit && feminineUnits.includes(unit) ? "ère" : "er";
  } else {
    suffix = "ème";
  }
  return number + suffix;
};
console.log(ordinalNumber(0), ordinalNumber(1), ordinalNumber(1, { unit: "minute" }), ordinalNumber(2));

function concatLabels(values: string[]): string[] {
  return values.map((value, index) => "" + (index === 0 ? value : value.toLowerCase()));
}
console.log(concatLabels(["Ab", "Cd"]).join("|"));

function initialCaps<T extends string>(values: T[]): string[] {
  return values.map(value => value[0].toUpperCase());
}
console.log(initialCaps(["ab", "cd"]).join("|"));

const unknownOne: unknown = 1;
const unknownZero: unknown = 0;
const fallback: Date = new Date(0);
const selected: unknown = unknownOne ? unknownOne : fallback;
const selectedFallback: unknown = unknownZero ? unknownZero : fallback;
console.log(typeof selected, selected === 1, selectedFallback instanceof Date);

function fold(value: any): number {
  return value.match((ok: number) => ok + 1, (_err: string) => 0);
}
console.log(fold({ match: (ok: (x: number) => number, _err: (s: string) => number) => ok(4) }));

function splitChars(value: string): string[] {
  return value.split("");
}
function splitNegative(value: string): string[] {
  return value.split(",", -1);
}
console.log(splitChars("abc").join("|"), splitChars("").length, splitNegative("a,b,").length, "a,b,c".split(",", 2).join("|"), "a,b".split(",", 0).length);

function kindOf(value: unknown): string {
  return typeof value;
}
console.log(kindOf(1), kindOf("s"), kindOf(undefined), kindOf(null), kindOf({}), kindOf(() => 1));

function project(values: string[], mapper: (value: unknown) => unknown): unknown[] {
  return values.map(mapper);
}
console.log(project(["aa", "b"], value => (value as string).length).join("|"));

const words = ["apple", "banana"];
console.log(words.map(word => word.startsWith("a")).join("|"));
const erasedWords: any[] = ["xax", "y"];
console.log(erasedWords.map(value => value.endsWith("x")).join("|"));

function pick(parts: string[], index: number): string | null {
  const p: string = parts[index];
  const nextP: string | undefined = parts[index + 1];
  return nextP === undefined && p && p.indexOf("*") === p.length - 1 ? p : null;
}
console.log(pick(["ab*"], 0), pick(["ab"], 0), pick([""], 0), pick(["a*", "b"], 0));
