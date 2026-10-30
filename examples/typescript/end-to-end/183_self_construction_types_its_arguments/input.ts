// A class constructed inside its own body (a clone method) types its
// constructor arguments from the constructor's declared parameters, so the
// options object is the options record, not an erased map (Hono's #clone).
interface Router<T> {
  name: string
  add(method: string, path: string, handler: T): void
  match(method: string, path: string): T[]
}
class ListRouter<T> implements Router<T> {
  name = 'list'
  items: T[] = []
  add(method: string, path: string, handler: T): void {
    this.items.push(handler)
  }
  match(method: string, path: string): T[] {
    return this.items
  }
}
type GetPath<E> = (request: string, options?: { env?: E }) => string
type Options<E> = { strict?: boolean; router?: Router<string>; getPath?: GetPath<E> }

class App<E = string> {
  router: Router<string>
  getPath: GetPath<E>
  constructor(options: Options<E> = {}) {
    this.router = options.router ?? new ListRouter<string>()
    this.getPath = options.getPath ?? ((r) => r)
  }
  clone(): App<E> {
    return new App<E>({ router: this.router, getPath: this.getPath })
  }
}
const a = new App()
const b = a.clone()
b.router.add('GET', '/x', 'h')
console.log(b.router.match('GET', '/x').join(','), a.router.match('GET', '/x').join(','))
