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
