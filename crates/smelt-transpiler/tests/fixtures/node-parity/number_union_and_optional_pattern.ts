// An integer-valued expression (`Date.now()`, a `length`) is a JavaScript
// number, so it fills a union's `number` arm; and `s.match(p)` with an absent
// pattern matches like `new RegExp(undefined)`, the empty pattern.
type DateArg = Date | number | string;

function describe(value: DateArg): string {
  if (typeof value === "number") return `number ${value >= 0}`;
  if (typeof value === "string") return `string ${value}`;
  return `date ${value.getUTCFullYear()}`;
}

function firstMatch(text: string, patterns: { [width: string]: RegExp }, width?: string): string {
  const pattern = (width && patterns[width]) || patterns["missing"];
  const found = text.match(pattern);
  return found ? `[${found[0]}] at ${found.index}` : "no match";
}

function main(): void {
  console.log(describe(Date.now()));
  console.log(describe("abc".length));
  console.log(describe("2024"));
  console.log(describe(new Date(Date.UTC(1999, 5, 1))));
  const patterns = { short: /^\d/, long: /^\d+/ };
  console.log(firstMatch("123abc", patterns, "long"));
  console.log(firstMatch("123abc", patterns, "short"));
  console.log(firstMatch("abc", patterns, "long"));
  console.log(firstMatch("abc", patterns));
}

main();
