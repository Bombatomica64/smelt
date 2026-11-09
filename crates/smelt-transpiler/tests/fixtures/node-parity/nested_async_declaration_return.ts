// A nested `async function` with an inferred return yields a Promise, so `.then`
// and `await` see the awaited value, including through a forward reference.
const main = async (): Promise<void> => {
  async function named() {
    return "n";
  }
  const b = await named().then((s) => s + "!");
  console.log(b);
  const repository = () => ({ create: async (x: number) => helper(x).then((v) => v + 1) });
  console.log(await repository().create(1));
  async function helper(x: number) {
    return x * 2;
  }
  const fail = async (): Promise<number> => {
    throw new Error("boom");
  };
  console.log(await fail().then(() => "ok", (e: Error) => e.message));
  try {
    await fail().then(() => 1);
  } catch (e) {
    console.log("caught", (e as Error).message);
  }
};
await main();
function top() {
  async function n2() {
    return 1;
  }
  return n2();
}
await top().then((v) => console.log(v));
