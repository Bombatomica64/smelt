// A rest-parameter arrow stored in a fixed-arity, overloaded callable slot
// receives every positional argument, and spreading its `args` (through a
// type assertion that is erased at run time) binds them one by one.

interface Respond {
  (data: string, detail: number): string;
  (data: string, detail: string): string;
}

class Ctx {
  #respond(data: string, detail: number | string): string {
    return typeof detail === "number" ? `${data}:${detail}` : `${data}:${detail.toUpperCase()}`;
  }

  direct(data: string, note?: string): string {
    return `${data}/${note ?? "-"}`;
  }

  get respond(): Respond {
    return (...args) => this.#respond(...(args as Parameters<Respond>));
  }
}

const ctx = new Ctx();
const respond = ctx.respond;
console.log(respond("a", 301), respond("b", "gone"));

// A list spread into a method binds its elements positionally, and an
// element past the end of the list is the absent case of its parameter.
const one: string[] = ["c"];
const two: string[] = ["d", "e"];
console.log(ctx.direct(...(one as [string])), ctx.direct(...(two as [string, string])));
