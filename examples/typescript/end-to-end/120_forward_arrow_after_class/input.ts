// A top-level arrow that is referenced before its declaration is lifted into
// an item ahead of source order, so the function above that calls it reaches
// the real item. The lift must still come AFTER the classes its body reads: a
// class's field and method signatures exist only once the class declaration
// is lowered. Lifted above `Request`, `req.all()` resolved against nothing and
// was typed as the receiver itself, so `delete headers[..]` saw a `Request`
// instead of a record (Hono's `cloneRawRequest` calling `req.header()`).

class Request {
  private headers: Record<string, string> = { 'content-length': '12', accept: 'json' }

  all(): Record<string, string> {
    return { ...this.headers }
  }

  header(name: string): string | undefined {
    return this.headers[name]
  }
}

async function report(req: Request): Promise<number> {
  return withoutLength(req, true)
}

export const withoutLength = async (req: Request, strip: boolean): Promise<number> => {
  const headers = req.all()
  if (strip) {
    delete headers['content-length']
  }
  console.log(req.header('accept') ?? 'none')
  return Object.keys(headers).length
}

const req = new Request()
console.log(await report(req))
console.log(await withoutLength(req, false))
