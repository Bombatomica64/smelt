// An unannotated callback whose body inference fails returns the type the
// TypeScript checker inferred.
//
// A callback handed to a parameter typed `any` has no contextual signature,
// so it is lowered with a "no hint" return and its body infers one. When that
// inference failed (an `async` body) the return erased, and every caller read
// the callback's result through `SmeltUnknown`. The checker inferred that
// return; it now stands in for the missing annotation.
//
// The body's own inference still runs FIRST: a callback Smelt already types
// keeps its type (the promise below resolves a record the checker would only
// call `unknown`), and the checker answers only a return the body left erased.

function keep(callback: any): any {
  return callback;
}

function run(callback: any, value: number): any {
  return callback(value);
}

async function main(): Promise<void> {
  const isEven = keep(async (n: number) => n % 2 === 0);
  console.log(await isEven(4), await isEven(5));

  const lengths = keep(async (word: string) => {
    const parts = word.split(",");
    return parts.map((part) => part.length);
  });
  const measured = await lengths("ab,c,def");
  console.log(measured.length, measured[2] + 1);

  const label = keep(async (n: number) => {
    await Promise.resolve();
    return `n=${n}`;
  });
  console.log((await label(4)).length);

  const late = keep(() => {
    return new Promise((resolve) => {
      setTimeout(() => resolve({ status: 200 }));
    });
  });
  console.log(JSON.stringify(await late()));

  console.log(await run(async (n: number) => n * 3, 7));
}

main();
