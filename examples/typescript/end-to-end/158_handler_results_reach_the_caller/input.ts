// A router's match result holds handlers typed as a UNION of function types
// (`Handler | Middleware`), reached through element reads; a context object
// builds a `Response` in a handler and a middleware edits its headers after
// `await next()`. Wildcard routes are matched with a regex produced by a
// function call. Every one of these results must reach the caller.

type Next = () => Promise<void>
type HandlerResponse = Response | Promise<Response> | Promise<void>
type Handler<R extends HandlerResponse = any> = (c: Ctx, next: Next) => R
type Middleware<R extends HandlerResponse = Response> = (c: Ctx, next: Next) => Promise<R | void>
type H<R extends HandlerResponse = any> = Handler<R> | Middleware<R>
type MatchResult = [[H, string][]]

class Ctx {
  finalized = false
  #res: Response | undefined

  get res(): Response {
    return (this.#res ||= new Response(null))
  }

  set res(res: Response | undefined) {
    this.#res = res
    this.finalized = true
  }

  text(body: string, status?: number): Response {
    return new Response(body, { status: status ?? 200 })
  }

  header(name: string, value: string): void {
    this.res.headers.set(name, value)
  }
}

function wildcard(path: string): RegExp {
  return new RegExp('^' + path.replace('*', '.*') + '$')
}

function matchRoutes(routes: [string, H][], path: string): MatchResult {
  const found: [H, string][] = []
  for (const [pattern, handler] of routes) {
    if (wildcard(pattern).test(path)) {
      found.push([handler, pattern])
    }
  }
  return [found]
}

async function dispatch(result: MatchResult, c: Ctx): Promise<Response> {
  if (result[0].length === 1) {
    // One handler: call it straight out of the match result.
    const res = result[0][0][0](c, async () => {})
    return res instanceof Promise ? ((await res) || c.res) : res
  }
  let index = -1
  const run = async (i: number): Promise<void> => {
    index = i
    const entry = result[0][i]
    if (!entry) {
      return
    }
    const res = await entry[0](c, () => run(i + 1))
    if (res && !c.finalized) {
      c.res = res
    }
  }
  await run(0)
  console.log('handlers run', index)
  return c.res
}

async function main(): Promise<void> {
  const text: H = (c) => c.text('root')
  const r1 = await dispatch(matchRoutes([['/a', text]], '/a'), new Ctx())
  console.log(r1.status, await r1.text())

  const created: H = async (c) => c.text('made', 201)
  const r2 = await dispatch(matchRoutes([['/b', created]], '/b'), new Ctx())
  console.log(r2.status, await r2.text())

  const poweredBy: H = async (c, next) => {
    await next()
    c.header('X-Powered-By', 'Smelt')
  }
  const r3 = await dispatch(matchRoutes([['*', poweredBy], ['/c', text], ['/d', created]], '/c'), new Ctx())
  console.log(r3.status, r3.headers.get('X-Powered-By'), await r3.text())

  const table: any[] = [(a: number, b: number) => a + b]
  console.log('erased element call', table[0](2, 3))
}

main()
