// `reduceRight` folds from the last element to the first while each callback
// still sees the element's original index; module consts keep the tuple and
// list shapes their annotations declare when a function body reads them.
type ParamAssocArray = [string, number][]

const paths: Record<string, [number, ParamAssocArray]> = {
  '/users/:id/posts/:post': [0, [['id', 0], ['post', 1]]],
  '/tags/:tag': [1, [['tag', 2]]],
}
const replacement: number[] = [3, 1, 2]

function paramIndexMap(path: string): Record<string, number> {
  const pathData = paths[path]
  const seed: Record<string, number> = {}
  return pathData[1].reduceRight((map, [key], i) => {
    map[key] = replacement[pathData[1][i][1]] * 10 + i
    return map
  }, seed)
}

function visitOrder(items: string[]): string {
  return items.reduceRight((acc, item, i) => acc + item + i, '')
}

function forwardOrder(items: string[]): string {
  return items.reduce((acc, item, i) => acc + item + i, '')
}

function lastMinusRest(values: number[]): number {
  return values.reduceRight((acc, value) => acc - value)
}

function handlerIndex(path: string): number {
  return paths[path][0]
}

console.log(JSON.stringify(paramIndexMap('/users/:id/posts/:post')))
console.log(JSON.stringify(paramIndexMap('/tags/:tag')))
console.log(visitOrder(['a', 'b', 'c']))
console.log(forwardOrder(['a', 'b', 'c']))
console.log(lastMinusRest([1, 2, 10]))
console.log(handlerIndex('/tags/:tag'))
console.log(replacement.map((value, i) => value + i).join(','))
