// Joins, absence and host data reads — blocker-logs/hono-v5-phase4.md.
//
// Four general rules Hono v5's core (`hono-base.ts`, `router/reg-exp-router`)
// needed, each checked against Node line by line:
//
// 1. A ternary whose consequent is `null` does not hint its alternate:
//    `c ? null : d ? ['a'] : ['b']` must lower the nested array literals as
//    arrays, not against `null`'s type (they were bound to `()` locals).
// 2. `never` is the bottom type of a function join: `c ? rethrow : handle` is
//    a callable returning what `handle` returns. It used to take `rethrow`'s
//    signature, so `handle`'s concrete result was returned through an erased
//    one.
// 3. `x || y` over an optional PRIMITIVE is a truthiness test: a present
//    `''`, `0` or `false` takes the fallback. It was lowered as presence
//    (`unwrap_or`), the `??` rule.
// 4. A modeled host class's data properties read through an optional chain
//    (`res?.status`, `res.body?.locked`) keep their concrete types, like the
//    direct reads do. `locked` is the body's consumed state: Smelt models no
//    reader API, so a stream is locked exactly when its body was read.

// 1.
function pick(token: string, next: string, prev: string): string {
  const pattern =
    token === '*'
      ? next[0] !== '/'
        ? null
        : prev === '/'
          ? ['', '', 'label']
          : ['', '', 'prefixed']
      : null
  return pattern ? pattern[2] : 'none'
}
console.log(pick('*', '/x', '/'), pick('*', '/x', 'a'), pick('*', 'x', '/'), pick('x', '', ''))

// 2.
const rethrow = (err: unknown): never => {
  throw err
}
const handle = (err: unknown): number => String(err).length
function chooseHandler(strict: boolean): (err: unknown) => number {
  const handler = strict ? rethrow : (err: unknown) => handle(err)
  return handler
}
console.log(chooseHandler(false)('four'))
try {
  chooseHandler(true)('boom')
} catch (error) {
  console.log('rethrown', error)
}

// 3.
function labelOf(parts: string[], index: number): string {
  return parts[index] || 'fallback'
}
function portOf(ports: number[]): number {
  return ports[0] || 80
}
console.log(labelOf(['a', '', 'c'], 0), labelOf(['a', '', 'c'], 1), labelOf(['a'], 5))
console.log(portOf([8080]), portOf([0]), portOf([]))

// 4.
async function responseFacts(): Promise<void> {
  const maybe: Response | undefined = new Response('hi', { status: 201, statusText: 'Made' })
  const missing: Response | undefined = undefined
  const status: number | undefined = maybe?.status
  console.log(status, maybe?.ok, maybe?.statusText, missing?.status)
  if (maybe) {
    console.log('locked before', maybe.body?.locked, maybe.bodyUsed)
    await maybe.text()
    console.log('locked after', maybe.body?.locked, maybe.bodyUsed)
  }
}
responseFacts()
