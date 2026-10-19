// JSON enters typed code at its first declared type, and is decoded there.
//
// `JSON.parse` is typed `any`, so the parsed value becomes typed at the first
// declared type it flows into: an annotation, a parameter, a return type,
// `as T`, `JSON.parse<T>`, or an awaited `response.json()` / `request.json()`.
// Smelt decodes the JSON into that type's concrete Rust storage right there
// (a struct, a tagged union enum, a list, a tuple, a record) instead of keeping
// a dynamic value -- design adapted from Bun's `--aot` typed entry checks.
//
// Every line below is JSON that MATCHES its type, so the output is diffed
// against Node line for line. JSON that does not match throws a catchable
// `TypeError` under smelt (`expected number, got string "3" at .visits`) where
// Node would carry the wrong value on silently; that intentional divergence is
// exercised by the runtime tests (`json_not_matching_a_declared_type_throws_a_
// catchable_type_error`) rather than here, because Node cannot print it.
interface Circle { kind: 'circle'; radius: number }
interface Square { kind: 'square'; side: number }
type Shape = Circle | Square
type Priority = 'low' | 'high'

interface Tree {
  label: string
  children: Tree[]
}

interface Order {
  id: number
  note?: string
  priority: Priority
  shapes: Shape[]
  totals: Record<string, number>
  range: [number, number]
  tree: Tree
}

function area(shape: Shape): number {
  if (shape.kind === 'circle') {
    return Math.round(Math.PI * shape.radius * shape.radius)
  }
  return shape.side * shape.side
}

function countNodes(tree: Tree): number {
  let total = 1
  for (const child of tree.children) {
    total += countNodes(child)
  }
  return total
}

function loadOrder(text: string): Order {
  return JSON.parse(text)
}

function double(value: number): number {
  return value * 2
}

const text =
  '{"id":42,"priority":"high","shapes":[{"kind":"circle","radius":2},{"kind":"square","side":3}],' +
  '"totals":{"net":10,"tax":2},"range":[1,5],' +
  '"tree":{"label":"root","children":[{"label":"a","children":[]},{"label":"b","children":[{"label":"c","children":[]}]}]},' +
  '"ignored":"undeclared properties are dropped"}'

const order = loadOrder(text)
console.log(order.id, order.priority, order.note === undefined)
console.log(order.shapes.map(area).join(','))
console.log(Object.keys(order.totals).join('+'), order.totals['net'] + order.totals['tax'])
console.log(order.range[1] - order.range[0], countNodes(order.tree))

const noted: Order = JSON.parse(text.replace('"id":42', '"id":7,"note":"rush"'))
console.log(noted.id, noted.note)

console.log(double(JSON.parse('21')))
const pair = JSON.parse('["left",true]') as [string, boolean]
console.log(pair[0], pair[1])
const counts = JSON.parse<number[]>('[3,1,2]')
console.log(counts.length, counts[0])

async function readBodies(): Promise<void> {
  const fromResponse: Circle = await new Response('{"kind":"circle","radius":1}').json()
  console.log(fromResponse.kind, fromResponse.radius)
  const request = new Request('http://example.test/', { method: 'POST', body: '{"kind":"square","side":4}' })
  const fromRequest: Square = await request.json()
  console.log(area(fromRequest))
}

await readBodies()
