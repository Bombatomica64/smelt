// `Promise.allSettled` answers one `PromiseSettledResult<T>` per input: the
// tagged union of `{ status: "fulfilled"; value: T }` and
// `{ status: "rejected"; reason: unknown }`, discriminated by `status`. The
// fulfilled `value` keeps the input's type; iterating the tuple an array
// literal answers reads its entries one by one.
async function ok(n: number): Promise<number> {
  return n * 2;
}
async function bad(msg: string): Promise<number> {
  throw new Error(msg);
}

function describe(entry: PromiseSettledResult<number>): string {
  return entry.status === "fulfilled" ? "value " + (entry.value + 1) : "reason " + (entry.reason instanceof Error ? "error" : "other");
}

async function main(): Promise<void> {
  const results = await Promise.allSettled([ok(1), bad("boom"), ok(3)]);
  for (const r of results) {
    if (r.status === "fulfilled") {
      console.log("ok", r.value + 1);
    } else {
      console.log("rejected", (r.reason as Error).message);
    }
  }
  const listed: Promise<number>[] = [ok(5), bad("late")];
  const settled = await Promise.allSettled(listed);
  console.log(settled.filter((s) => s.status === "fulfilled").length, settled.length);
  console.log(settled.map(describe).join(" | "));
}
main();
