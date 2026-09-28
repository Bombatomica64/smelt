// Class methods reached through a GETTER, called inside closure bodies.
//
// A read of `ctx.req` where `req` is `get req(): Request` is a value of the
// getter's declared type, so `ctx.req.header(..)` dispatches to the class
// method directly — in a function body, in a local arrow, and in an arrow
// passed as a callback — and the getter runs once per read.

type KnownHeader = "accept" | "content-type";

class Request {
  private headers: Map<string, string>;

  constructor(headers: Map<string, string>) {
    this.headers = headers;
  }

  // An overloaded method, spelled like Hono's `HonoRequest.header`.
  header(name: KnownHeader): string | undefined;
  header(name: string): string | undefined;
  header(name: string): string | undefined {
    return this.headers.get(name);
  }

  names(): string[] {
    return [...this.headers.keys()];
  }

  has(name: string): boolean {
    return this.headers.has(name);
  }
}

class Context {
  reads = 0;
  #req: Request | undefined;
  #raw: Map<string, string>;

  constructor(raw: Map<string, string>) {
    this.#raw = raw;
  }

  // A lazily-built getter with a side effect, so every read is observable.
  get req(): Request {
    this.reads += 1;
    this.#req ??= new Request(this.#raw);
    return this.#req;
  }

  text(body: string): string {
    return "text:" + body;
  }
}

// A function body.
function contentType(c: Context): string {
  return c.req.header("content-type") ?? "none";
}

// A local arrow whose body calls the method through the getter.
const handler = (c: Context) => {
  const accept = c.req.header("accept") || "";
  return c.text(accept);
};

// A local arrow nested in a function, and a predicate callback.
function present(c: Context, names: string[]): string[] {
  const known = (name: string) => c.req.has(name);
  return names.filter((name) => c.req.has(name) && known(name));
}

const ctx = new Context(
  new Map([
    ["content-type", "text/plain"],
    ["accept", "*/*"],
  ]),
);
console.log(contentType(ctx));
console.log(ctx.reads);
console.log(handler(ctx));
console.log(ctx.reads);
console.log(present(ctx, ["accept", "x-missing", "content-type"]).join(","));
console.log(ctx.reads);
console.log(ctx.req.names().join(","));
console.log(ctx.reads);
