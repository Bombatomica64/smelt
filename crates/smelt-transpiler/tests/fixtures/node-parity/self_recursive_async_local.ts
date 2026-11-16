// A self-recursive async local function keeps itself alive while a call of
// it is pending: `dispatch(i + 1)` runs from a later job, after the frame that
// defined `dispatch` has returned its first promise (hono's `compose`).
type Handler = (log: string[], next: () => Promise<void>) => Promise<void>;
function compose(handlers: Handler[]): (log: string[]) => Promise<string[]> {
  return (log) => {
    let index = -1;
    return dispatch(0);
    async function dispatch(i: number): Promise<string[]> {
      if (i <= index) throw new Error("next() called multiple times");
      index = i;
      const handler = handlers[i];
      if (handler) await handler(log, () => dispatch(i + 1).then(() => undefined));
      return log;
    }
  };
}
const run = compose([
  async (log, next) => { log.push("a1"); await null; await next(); log.push("a2"); },
  async (log, next) => { log.push("b1"); await next(); log.push("b2"); },
  async (log, next) => { log.push("c"); await next(); },
]);
run([]).then((log) => console.log(log.join(",")));
const twice = compose([async (_log, next) => { await next(); await next(); }]);
twice([]).catch((error: Error) => console.log("caught", error.message));
