// A rest parameter typed as a fixed-width tuple is fixed parameters: each
// element keeps its own type, and the rest name is the tuple of them.

function pair(...args: [string, number?]): string {
  return `${args[0]}:${args[1] ?? 0}`;
}

const triple = (...args: [string, number, boolean]): string => `${args[0]}${args[1]}${args[2]}`;

class Box {
  put(...args: [string, number]): string {
    return args[0].repeat(args[1]);
  }
}

// An unannotated rest parameter in a fixed-arity slot is the tuple of the
// slot's parameters, as TypeScript types it.
type Handler = (name: string, count: number) => string;
const handler: Handler = (...args) => pair(...args);
const forward: Handler = (...args) => `${args.length}:${args[0]}`;

const values: [string, number] = ["z", 2];
console.log(pair("a"), pair("b", 3), triple("x", 1, true), new Box().put("ab", 2));
console.log(handler("h", 4), forward("f", 1), pair(...values));

// Spreading the tuple through an assertion to a NARROWER parameter list still
// passes every element: the assertion is erased at run time.
interface Respond {
  (data: string, status?: number, note?: string): string;
  (data: string, status?: number): string;
}
class Ctx {
  #respond(data: string, status?: number, note?: string): string {
    return `${data}:${status ?? 200}:${note ?? "-"}`;
  }
  get respond(): Respond {
    return (...args) => this.#respond(...(args as Parameters<Respond>));
  }
}
const ctx = new Ctx();
console.log(ctx.respond("a"), ctx.respond("b", 301), ctx.respond("c", 302, "x"));
