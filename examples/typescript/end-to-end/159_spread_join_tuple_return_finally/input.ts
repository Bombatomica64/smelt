// Spread joins, tuple returns, bitwise operands and `finally` —
// blocker-logs/hono-v5-phase4.md (Hono v5 `helper/cookie`, `utils/accept`,
// `utils/cookie`, `compose`).
//
// 1. An object literal with spreads keeps an OPTIONAL record source's
//    properties: `{ path: '/', ...opt, domain: 'example.com' }`. (The
//    heterogeneous-values join — `..., secure: true` — makes the literal the
//    erased record, so it is checked by the frontend unit test
//    `unhinted_spread_with_heterogeneous_values_keeps_its_sources` instead.)
// 2. A tuple returned at a wider tuple type is converted element by element
//    (`[n, accept] as [number, Accept]` returned as
//    `[number, Accept | undefined]`).
// 3. Bitwise operators apply `ToNumber` to an erased (JSON) operand.
// 4. `promise.finally(cb)` runs `cb` and keeps the original outcome.
// 5. A `catch` parameter read by code the rejoined `try` continuation emits
//    once is the caught value (the arm used to shadow it with a `let`).

// 1.
function withDefaults(opt?: Record<string, string>): string {
  const merged = { path: '/', ...opt, domain: 'example.com' }
  return merged.path + ' ' + merged.domain
}
console.log(withDefaults({ path: '/app' }), withDefaults())

// 2.
interface Accept {
  type: string
  q: number
}
function nextAccept(header: string): [number, Accept | undefined] {
  const accept: Accept = { type: '', q: 1 }
  if (header === '') {
    return [0, undefined]
  }
  accept.type = header
  return [header.length, accept] as [number, Accept]
}
const [consumed, accept] = nextAccept('json')
console.log(consumed, accept?.type, nextAccept('')[1] === undefined)

// 3.
console.log(JSON.parse('{"maxAge": 12.7}').maxAge | 0, JSON.parse('{"maxAge": "7"}').maxAge | 0)

// 4.
let cleanups = 0
async function work(n: number): Promise<number> {
  return n * 2
}
// Sections 4 and 5 run on ONE chain: Smelt's cooperative scheduler does not
// reproduce Node's interleaving of independent promise chains.
function finallyThenCatch(): Promise<void> {
  return work(21)
    .finally(() => {
      cleanups += 1
    })
    .then((value) => {
      console.log('finally kept', value, cleanups)
      return caught()
    })
    .then((value) => console.log(value))
}

// 5.
class Failure {
  message: string
  constructor(message: string) {
    this.message = message
  }
}
async function failing(): Promise<void> {
  throw new Failure('next() called multiple times')
}
function prepare(n: number): number {
  if (n < 0) throw new Failure('negative')
  return n
}
async function caught(): Promise<string> {
  let seen = 'none'
  try {
    const n = prepare(1)
    await failing()
    seen = 'unreachable ' + n
  } catch (err) {
    seen = err instanceof Failure && /multiple times/.test(err.message) ? 'caught it' : 'wrong value'
  }
  return seen
}
finallyThenCatch()
