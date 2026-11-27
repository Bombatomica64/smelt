// A generic function whose callback parameter is OWNED (held across an
// `await`, or stored) keeps its type parameter: `firstOf<T>` is generic over
// `T` and each call adapts its arrow to the instantiated slot.
async function firstOf<T>(xs: T[], f: (x: T) => Promise<boolean>): Promise<T | undefined> {
  for (const x of xs) {
    if (await f(x)) return x;
  }
  return undefined;
}

async function countAsync<T>(xs: T[], f: (x: T, i: number) => Promise<boolean>): Promise<number> {
  let n = 0;
  for (let i = 0; i < xs.length; i++) {
    if (await f(xs[i], i)) n++;
  }
  return n;
}

// A lifted caller forwarding its own `T` to a lifted callee.
async function anyOf<T>(xs: T[], f: (x: T) => Promise<boolean>): Promise<boolean> {
  return (await firstOf(xs, f)) !== undefined;
}

// A stored callback (owned without any `async`).
function memoize<T>(seed: T, f: (x: T) => string): () => string {
  return () => f(seed) + "!";
}

async function main(): Promise<void> {
  console.log(await firstOf([1, 2, 3], async (x) => x > 1));
  console.log(await firstOf(["a", "bb", "ccc"], async (s) => s.length > 5));
  console.log(await countAsync([3, 4, 5, 6], async (x, i) => x % 2 === 0 || i === 0));
  console.log(await anyOf(["x", "yy"], async (s) => s.length === 2));
  console.log(memoize(21, (n) => String(n * 2))());
}

main();
