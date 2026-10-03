// A module function declared with an explicit `this` parameter, installed as a
// class field and reached through an interface view of the instance.

interface Router {
  name: string
  add(method: string, path: string, handler: string): void
  match(method: string, path: string): string[]
}

// Reads its table off the receiver on the first call, then replaces itself on
// that receiver with a closure over the table.
function match<R extends Router>(this: R, method: string, path: string): string[] {
  const table: Record<string, string[]> = (this as any).table
  const none: string[] = []
  const lookup = (method: string, path: string): string[] => table[method + ' ' + path] ?? none
  this.match = lookup
  console.log('first match', Object.keys(table).length)
  return lookup(method, path)
}

class TableRouter implements Router {
  name: string = 'TableRouter'
  table: Record<string, string[]> = {}
  add(method: string, path: string, handler: string): void {
    const k = method + ' ' + path
    if (!this.table[k]) this.table[k] = []
    this.table[k].push(handler)
  }
  match: typeof match = match
  count(pairs: [string, string][]): number {
    let n = 0
    // `lookup` is also an arrow inside `match` above; this binding is a
    // different variable and must not resolve to it.
    for (const [lookup, p] of pairs) {
      n += (lookup + p).length
    }
    return n
  }
}

// Delegates to its first router through the `Router` interface, then rebinds
// its own `match` to that router's current one.
class SmartRouter implements Router {
  name: string = 'SmartRouter'
  #routers: Router[]
  constructor(routers: Router[]) {
    this.#routers = routers
  }
  add(method: string, path: string, handler: string): void {
    for (const r of this.#routers) r.add(method, path, handler)
  }
  match(method: string, path: string): string[] {
    const router = this.#routers[0]
    const res = router.match(method, path)
    this.match = router.match.bind(router)
    this.name = 'SmartRouter + ' + router.name
    return res
  }
}

function greet(this: { prefix: string }, name: string): string {
  return this.prefix + name
}

const r = new TableRouter()
r.add('GET', '/a', 'one')
r.add('GET', '/a', 'two')
r.add('POST', '/b', 'three')
console.log(r.match('GET', '/a').join(','))
console.log(r.match('POST', '/b').join(','))
console.log(r.match('GET', '/zz').length)
console.log(r.count([['ab', 'c']]))

const s = new SmartRouter([new TableRouter()])
s.add('GET', '/x', 'hx')
console.log(s.match('GET', '/x').join(','))
console.log(s.match('GET', '/x').join(','))
console.log(s.match('GET', '/y').length)
console.log(s.name)

const bound = greet.bind({ prefix: 'hi ' })
console.log(bound('bob'))
const bound2 = greet.bind({ prefix: 'yo ' }, 'amy')
console.log(bound2())

// Handlers installed under computed keys, registering into an optional list
// that a guard proves present.
const VERBS = ['get', 'post'] as const

class App {
  get!: (path: string) => number
  post!: (path: string) => number
  #routes?: string[] = []
  constructor() {
    const verbs = [...VERBS]
    verbs.forEach((verb) => {
      this[verb] = (path: string) => this.#register(verb.toUpperCase() + ' ' + path)
    })
  }
  #register(route: string): number {
    if (!this.#routes) {
      throw new Error('sealed')
    }
    this.#routes.push(route)
    return this.#routes.length
  }
  routes(): string {
    return (this.#routes ?? []).join(', ')
  }
}

const app = new App()
console.log(app.get('/a'), app.post('/b'), app.get('/c'))
console.log(app.routes())
