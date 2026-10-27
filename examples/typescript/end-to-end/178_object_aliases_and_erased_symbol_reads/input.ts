// An object-shaped type alias is the same structural type an interface is,
// so it gets the same generated struct, generic defaults included. A symbol
// member read through a type-only `as unknown` is the member itself, and a
// throwing getter inside `try` reaches the `catch`.

type Options = { label?: string; limit?: number; tags?: string[] };
type Tagged<T = string, N extends number = number> = { value: T; count: N };

function configure(options?: Options): string {
  return `${options?.label ?? "default"}/${options?.limit ?? 3}`;
}

function first(tagged: Tagged): string {
  return `${tagged.value}x${tagged.count}`;
}

const MATCH: unique symbol = Symbol();

class Request {
  #routes: [string, string][];
  #context: string | undefined;
  constructor(routes: [string, string][], context?: string) {
    this.#routes = routes;
    this.#context = context;
  }
  get [MATCH](): [[string, string][]] {
    return [this.#routes];
  }
  get context(): string {
    if (this.#context) {
      return this.#context;
    }
    throw new Error("no context");
  }
}

function paths(req: Request): string {
  // @ts-expect-error MATCH is not part of the public surface
  return (req as unknown)[MATCH][0].map(([, path]) => path).join(",");
}

function contextOf(req: Request): string {
  let context: string | undefined = undefined;
  try {
    context = req.context;
  } catch {}
  return context ?? "none";
}

console.log(configure(), configure({ label: "custom" }), configure({ limit: 9 }));
console.log(first({ value: "a", count: 2 }));
const req = new Request([["a", "/x"], ["b", "/y"]], "ctx");
const noRoutes: [string, string][] = [];
console.log(paths(req), contextOf(req), contextOf(new Request(noRoutes)));
