// Short-circuit chains inside a closure body rejoin ONE shared continuation.
//
// Each `&&` / `||` operand is a branch whose two arms meet again at the next
// test. The closure emitter used to emit that meeting point inside both arms,
// so a k-long chain emitted 2^k copies of everything after it (Hono's
// `trailing-slash` middleware: 158 source lines, 3 MB of Rust). The arms now
// end at the join and the rest of the closure body is emitted once, so the
// generated Rust is linear in the chain length. Early returns inside a
// branch, an `await` between two chains, and a `try`/`catch` in a closure
// take the same rule.

type Handler = (path: string, strict: boolean) => Promise<string>

function makeHandler(prefix: string): Handler {
  return async (path: string, strict: boolean): Promise<string> => {
    let score = 0
    if (
      path.length > 0 &&
      path.startsWith('/') &&
      !path.endsWith('/') &&
      path !== '/' &&
      !path.includes('..') &&
      path.length < 64 &&
      path.indexOf(prefix) === 0 &&
      path.charAt(1) !== '_'
    ) {
      score += 1
    }
    const settled = await Promise.resolve(score)
    if (strict || path === prefix || path.endsWith('.html') || path.endsWith('.json')) {
      score += 10
    }
    if (settled === 0 && !strict) {
      return `reject ${path}`
    }
    let parsed = 0
    try {
      parsed = check(path)
    } catch (error) {
      if (error instanceof Error) {
        parsed = -1
      }
    }
    return `${path} score=${score} parsed=${parsed}`
  }
}

function check(path: string): number {
  if (path.length > 10) {
    throw new Error('too long')
  }
  return path.length
}

function counter(): (n: number) => number {
  return (n: number): number => {
    let hits = 0
    if (n > 0 || n < -100 || n === -5) {
      hits += 1
    }
    if (n % 2 === 0 && n % 3 === 0 && n % 5 === 0) {
      return hits + 100
    }
    return hits
  }
}

async function run(): Promise<void> {
  const handler = makeHandler('/api')
  console.log(await handler('/api/users', false))
  console.log(await handler('/api/users/', false))
  console.log(await handler('/api', true))
  console.log(await handler('/api/a', true))
  console.log(await handler('relative', false))
  const count = counter()
  console.log(count(3), count(-5), count(30), count(-7), count(0))
}

run()
