// Module `const` containers written through, and unions of record types.
//
// * `const cache: Record<string, P> = {}` is a constant BINDING, not a
//   constant value: functions write through it (`cache[key] = p`,
//   `seen.push(x)`). The const-item path cloned its `{}` initializer into
//   every use site, so each write landed on a fresh empty record and every
//   read missed (Hono's `patternCache`). A module const container that is
//   written through anywhere is now a shared module slot, like a mutated
//   `let`.
// * `Record<string, string> | Record<string, string[]>` stores as ONE record
//   whose values are `string | string[]`; kept as two record arms, a push into
//   an array entry of the string-arm record read back "[object Object]"
//   (Hono's `getQueryParams`).
//
// Every line below is diffed against Node.

type Pattern = readonly [string, string, true]
const cache: { [key: string]: Pattern } = {}
const seen: string[] = []

export function getPattern(label: string): Pattern {
  if (!cache[label]) {
    cache[label] = [label, label.slice(1), true]
    seen.push(label)
  }
  return cache[label]
}

const a = getPattern(':id')
const b = getPattern(':id')
getPattern(':name')
console.log(a[1], b[0], seen.join('|'))

const collect = (
  multiple: boolean
): Record<string, string> | Record<string, string[]> => {
  const results: Record<string, string> | Record<string, string[]> = {}
  const pairs = [['name', 'hey'], ['name', 'ho'], ['x', '1']]
  for (const [name, value] of pairs) {
    if (multiple) {
      if (!(results[name] && Array.isArray(results[name]))) {
        results[name] = []
      }
      ;(results[name] as string[]).push(value)
    } else {
      results[name] ??= value
    }
  }
  return results
}
console.log(JSON.stringify(collect(true)), JSON.stringify(collect(false)))
