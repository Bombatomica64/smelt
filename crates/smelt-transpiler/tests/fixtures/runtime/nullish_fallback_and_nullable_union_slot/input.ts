// Two ways an absent value used to turn into a present one:
// * `a ?? b` is nullish exactly when `b` is. Typed by the other arm of an
//   enclosing ternary, `arg?.status ?? this.#status` lost `b`'s `undefined`
//   and became `NaN` (Hono's `c.json(x)` answered status NaN);
// * a union with a `null` arm passed to an optional slot was wrapped whole in
//   `Some(..)`, so `null` picked whichever arm accepts it: `new Response(null)`
//   through a `Data | null` parameter got a urlencoded content type.
type StatusCode = 200 | 201 | 404;
type Data = string | ArrayBuffer | ReadableStream | Uint8Array<ArrayBuffer>;

const createResponseInstance = (body?: BodyInit | null | undefined, init?: ResponseInit): Response =>
  new Response(body, init);

class Ctx {
  #status: StatusCode | undefined;
  setStatus(status: StatusCode): void {
    this.#status = status;
  }
  make(data: Data | null, arg?: StatusCode | ResponseInit): Response {
    const status = typeof arg === 'number' ? arg : (arg?.status ?? this.#status);
    return createResponseInstance(data, { status });
  }
}

const c = new Ctx();
const plain = c.make('a');
const created = c.make('b', 201);
const noContent: ResponseInit = { status: 204 };
const empty = c.make(null, noContent);
c.setStatus(404);
const missing = c.make('d');
console.log(plain.status, created.status, empty.status, missing.status);
console.log(String(empty.headers.get('content-type') ?? 'none'), String(plain.headers.get('content-type') ?? 'none'));
