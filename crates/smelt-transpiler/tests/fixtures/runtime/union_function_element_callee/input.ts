// A call whose callee is a tuple ELEMENT typed as a union of function types
// (`Handler | Middleware`) is a call: it must run the stored function. It used
// to lower to its first argument (and an `unknown` callee to `undefined`),
// which compiled and silently skipped the call — Hono's router dispatch never
// ran a route handler.
class Ctx {
  name = 'ctx';
}
type Next = () => Promise<void>;
type Handler = (c: Ctx, next: Next) => any;
type Middleware = (c: Ctx, next: Next) => Promise<string | void>;
type H = Handler | Middleware;

function callTuple(fs: [H, string][], c: Ctx): string {
  const res: ReturnType<H> = fs[0]![0](c, async (): Promise<void> => {});
  return String(res);
}

function callErased(fs: unknown[], c: Ctx): string {
  const f = fs[0] as (c: Ctx) => string;
  return f(c);
}

const handler: H = (c: Ctx): string => 'handled ' + c.name;
console.log(callTuple([[handler, '/x']], new Ctx()));
console.log(callErased([(c: Ctx): string => 'erased ' + c.name], new Ctx()));
