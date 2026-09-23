// An async arrow may return a resolved value on one path and a promise on
// another; both settle the arrow's own promise with the resolved value.
async function demo(): Promise<void> {
  const settle = async (value: number, pending?: Promise<number>): Promise<number> => {
    if (pending) {
      return pending;
    }
    return value * 2;
  };
  console.log(await settle(3));
  console.log(await settle(1, Promise.resolve(7)));
}

await demo();
