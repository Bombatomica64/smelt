"use strict";
// A rejection is reported only when no handler is attached by the time the
// job queue drains. A handler attached later in the same turn, inside a
// microtask, or by awaiting, counts; so does one attached to a derived
// promise down the chain. Typed and erased promises follow the same rule.
const caught: string[] = [];
const note = (error: Error) => {
  caught.push(error.message);
};
const host: any = {
  async fail(label: string) {
    throw new Error(label);
  },
};

const lateInTurn = host.fail("late in turn");
console.log("sync work");
lateInTurn.catch(note);

const lateInMicrotask = host.fail("late in microtask");
Promise.resolve().then(() => {
  lateInMicrotask.catch(note);
});

host
  .fail("down the chain")
  .then(() => "never")
  .catch(note);

const awaited = host.fail("awaited");
(async () => {
  try {
    await awaited;
  } catch (error) {
    note(error as Error);
  }
})();

async function typedFail(label: string): Promise<number> {
  throw new Error(label);
}
const typed = typedFail("typed");
typed.catch(note);

console.log("sync end");
// Runs after the job queue has drained, so after every handler above.
setTimeout(() => console.log("caught:", caught.sort().join(", ")), 0);
