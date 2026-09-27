// `String.prototype.match` answers the same match array as `RegExp.exec`.
//
// For a non-global regex the answer IS `exec`: a capture group that did not
// participate is `undefined`, while an empty group that did is `''`. A router
// that marks each alternative with an empty group `()` finds the matched one
// with `match.indexOf('', 1)`, which only works if the unmatched markers are
// `undefined`. The match is also an ordinary array: array methods, `for...of`,
// spread and destructuring all see its groups. A global regex answers the
// plain list of every whole match.

const routes = /^\/a(?:\/b()|\/c())$/

function pick(path: string): string {
  const m = path.match(routes)
  if (!m) return 'none'
  const index = m.indexOf('', 1)
  return `${index}:${m.length}:${m[1] === undefined ? 'u' : 'e'}`
}

console.log(pick('/a/b'), pick('/a/c'), pick('/x'))

function describe(text: string): void {
  const m = text.match(/(\d+)-(\d+)?/)
  if (!m) {
    console.log('no match')
    return
  }
  const rest = m.slice(1)
  console.log(rest.length, m.join('|'), m.map((group) => group ?? 'U').join(','))
  for (const group of m) {
    console.log(group === undefined ? 'undefined' : group)
  }
  const [, first, second] = m
  console.log([...m].length, m.index, m.input, first, second === undefined)
}

describe('12-')
describe('3-4')
describe('x')

function digits(text: string): string {
  const all = text.match(/\d/g)
  const joined = all ? all.join('+') : 'none'
  const tagged = (text.match(/\d/g) || []).map((digit) => `<${digit}>`).join('')
  return `${all?.length ?? 0} ${joined} ${tagged}`
}

console.log(digits('a1b22'), digits('ab'))

function firstTwo(text: string, pattern: RegExp): string {
  const groups = text.match(pattern) ?? []
  return groups.slice(0, 2).map((group) => group ?? '-').join('|')
}

console.log(firstTwo('a1', /([a-z])(\d)?/), firstTwo('b', /([a-z])(\d)?/), firstTwo('b', /z/))
