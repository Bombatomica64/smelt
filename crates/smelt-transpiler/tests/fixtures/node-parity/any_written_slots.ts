// A record whose elements the program writes with an `any` value holds what
// was written (ts-pattern's matchers collect selections into a
// `Record<string, unknown[]>` through a `(key, value: any)` selector).
function collect(entries: Array<[string, unknown]>): { selections: Record<string, unknown> } {
  const selections: Record<string, unknown[]> = {};
  const selector = (key: string, value: any) => {
    selections[key] = value;
  };
  for (const [key, value] of entries) {
    selector(key, value);
  }
  return { selections };
}

const picked = collect([
  ["absent", undefined],
  ["name", "n"],
  ["count", 3],
  ["list", [1, 2]],
]).selections;
console.log(picked.absent === undefined, "absent" in picked, picked.name, picked.count);
console.log(JSON.stringify(picked.list), Object.keys(picked).join(","));

// Returned by name through a declared narrower type: the function returns the
// same dynamic value, so the declared element type widens with the local.
function collectDeclared(entries: Array<[string, unknown]>): Record<string, unknown[]> {
  const selections: Record<string, unknown[]> = {};
  const selector = (key: string, value: any) => {
    selections[key] = value;
  };
  for (const [key, value] of entries) selector(key, value);
  return selections;
}
const declared = collectDeclared([["absent", undefined], ["name", "n"], ["list", [1, 2]]]);
console.log(declared.absent === undefined, declared.name, JSON.stringify(declared.list));

function fill(values: unknown[]): number[] {
  const out: number[] = [];
  values.forEach((value, index) => {
    out[index] = value as any;
  });
  return out;
}
const filled = fill(["a", 2, null]);
console.log(typeof filled[0], filled[1], filled[2] === null, filled.length);
