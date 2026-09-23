// A closure nested two levels deep may reference a local of its GRANDPARENT
// frame. Each intermediate closure must capture that local too, and every
// capture must name a binding of the frame that immediately encloses it —
// otherwise two different values collide on one source local id.

// Three levels: arrow -> arrow -> hoisted, self-recursive `function`
// declaration that uses the GRANDPARENT's parameter (`steps`) and the
// PARENT's parameter (`label`) at once.
const makeWalker = (steps: string[]) => (label: string): string => {
  return walk(0)
  function walk(i: number): string {
    if (i >= steps.length) {
      return label
    }
    return steps[i] + '>' + walk(i + 1)
  }
}

// The same nesting with a NON-recursive nested function declaration.
const makeJoiner = (parts: string[]) => (glue: string): string => {
  return join()
  function join(): string {
    let out = ''
    for (const part of parts) {
      out = out === '' ? part : out + glue + part
    }
    return out
  }
}

// The same nesting through arrows only, so the transitive capture rule is not
// specific to function declarations.
const makeTagger = (prefix: string) => (suffix: string) => (body: string) =>
  prefix + body + suffix

// A grandparent local that is a `const` of the outer body rather than a
// parameter, used only by the innermost frame.
const makeCounter = (start: number) => {
  const step = 2
  return (times: number): number => {
    let total = start
    for (let i = 0; i < times; i++) {
      total = total + step
    }
    return total
  }
}

console.log(makeWalker(['a', 'b', 'c'])('end'))
console.log(makeWalker([])('only'))
console.log(makeJoiner(['x', 'y', 'z'])('-'))
console.log(makeTagger('<')('>')('mid'))
console.log(makeCounter(10)(3))
