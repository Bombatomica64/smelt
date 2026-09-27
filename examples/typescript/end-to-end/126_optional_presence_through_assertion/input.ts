// `record[a] || record[b]` over object values is presence-coalescing, and a
// type assertion (`as`) is erased at runtime: it cannot make an absent entry
// present. The follow-up truthiness test must therefore stay a real check.
//
// The assertion here is also a DOWNCAST (`HandlerSet` -> `HandlerParamsSet`,
// which adds `params`): the added property is absent until the program assigns
// it, and a required-field interface literal (`{ handler, score }`) is built as
// the struct itself rather than as an erased record.

interface HandlerSet { handler: string; score: number }
interface HandlerParamsSet extends HandlerSet { params: Record<string, string> }

class Table {
  methods: Record<string, HandlerSet>[] = []

  add(method: string, handler: string, score: number): void {
    this.methods.push({ [method]: { handler, score } })
  }

  lookup(method: string): string[] {
    const found: string[] = []
    for (let i = 0; i < this.methods.length; i++) {
      const m = this.methods[i]
      const set = (m[method] || m['ALL']) as HandlerParamsSet
      if (set) {
        found.push(set.handler + ':' + set.score)
      }
    }
    return found
  }
}

const table = new Table()
table.add('get', 'get hello', 1)
table.add('post', 'post hello', 2)
table.add('ALL', 'any', 3)
console.log(table.lookup('get').join(','))
console.log(table.lookup('post').join(','))
console.log(table.lookup('put').join(','))

const entries: Record<string, HandlerSet> = { a: { handler: 'first', score: 1 } }
const picked = entries['missing'] || entries['a']
console.log(picked ? picked.handler : 'none')
const absent = entries['missing'] || entries['other']
console.log(absent ? absent.handler : 'none')

const base: HandlerSet = { handler: 'h', score: 1 }
const full = base as HandlerParamsSet
full.params = {}
full.params['k'] = 'v'
console.log(full.handler, full.score, full.params['k'])
