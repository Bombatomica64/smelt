// Fixture: async_generic_callback_awaited
// Area: dispatch
// Guards: an `async` generic that awaits its borrowed callback in a loop.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export async function h1<T>(xs: T[], cb: (v: T) => Promise<boolean>): Promise<number> {
  let n = 0;
  for (const x of xs) { if (await cb(x)) { n++; } }
  return n;
}
export async function use1(ns: number[]): Promise<number> { return await h1(ns, async (v: number) => v > 1); }
