// An unannotated closure returns the type the TypeScript checker inferred.
//
// A closure with no return annotation and no contextual return type was
// lowered with a "no hint" return so its body could infer one, and an async
// body returning a local Smelt could not follow erased it: the binding
// reached its callers as `fn(..) -> Future<SmeltUnknown>`, so the awaited
// record below lost its type, and so did a generic call's type parameter
// bound from the closure (`R` of `mapAll`). The checker inferred the return;
// it is used as if the source had annotated it.
//
// Every line below is diffed against Node.

function mapAll<T, R>(items: readonly T[], fn: (item: T) => R): R[] {
  return items.map(fn);
}

async function main(): Promise<void> {
  const doubled = async (accumulator: Record<string, number>, key: string) => {
    accumulator[key] = key.length * 2;
    return accumulator;
  };
  const result = await doubled(await doubled({}, "a"), "bb");
  console.log(JSON.stringify(result), result["bb"] + 1);

  const isEven = async (n: number) => n % 2 === 0;
  console.log(await isEven(4), await isEven(5));

  const scored = (word: string) => {
    const counts: Record<string, number> = {};
    for (const letter of word) {
      counts[letter] = (counts[letter] ?? 0) + 1;
    }
    return counts;
  };
  const tallies = mapAll(["aab", "c"], scored);
  console.log(tallies.length, tallies[0]["a"], JSON.stringify(tallies[1]));
}

main();
