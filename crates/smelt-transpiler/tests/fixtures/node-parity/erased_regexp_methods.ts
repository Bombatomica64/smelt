// A RegExp that reached erased dataflow still answers `RegExp.prototype`'s
// methods when read as a member (date-fns' `findKey(parsePatterns, (pattern:
// any) => pattern.test(s))` over a locale's pattern table).
function findIndex<Item>(items: Item[], predicate: (item: Item) => boolean): number {
  for (let i = 0; i < items.length; i++) {
    if (predicate(items[i])) return i;
  }
  return -1;
}

const patterns: Record<string, unknown> = {
  any: [/^(一|1(?!\d))/i, /^(十(?!(一|二))|10)/i, /^(十一|11)/i, /b/g],
};
const table = patterns["any"];
if (Array.isArray(table)) {
  console.log(findIndex(table, (pattern: any) => pattern.test("十月")));
  console.log(findIndex(table, (pattern: any) => pattern.test("十一月")));
  console.log(findIndex(table, (pattern: any) => pattern.test("12")));
  const global: any = table[3];
  const first = global.test("abba");
  const afterFirst = global.lastIndex;
  const second = global.test("abba");
  const afterSecond = global.lastIndex;
  const third = global.test("abba");
  const afterThird = global.lastIndex;
  console.log(first, afterFirst, second, afterSecond, third, afterThird);
  const match = (table[0] as any).exec("1月");
  console.log(match[0], match[1], match.index);
  console.log(String((table[1] as any).toString()), (table[2] as any).test(undefined));
}
