// A closure that is not `async` and returns a promise returns that same
// promise (no new promise, no extra job turn); an `async` one returns a new
// promise. Function items used as values forward their promise the same way.
async function value(n: number): Promise<number> {
  return n;
}
function later(): Promise<number> {
  return value(5);
}
const shared = value(1);
const forward = (): Promise<number> => shared;
const wrapped = async (): Promise<number> => shared;
console.log("same promise", forward() === shared, wrapped() === shared);
const host: any = { later };
host.later().then((v: unknown) => console.log("forwarded", v));
const asyncHost: any = { run: async () => 4 };
asyncHost.run().then((v: unknown) => console.log("async arrow", v));
Promise.resolve().then(() => console.log("plain job"));
const call = (f: () => Promise<number>) => f();
call(later).then((v) => console.log("through a callback", v));
