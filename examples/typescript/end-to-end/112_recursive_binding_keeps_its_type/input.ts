// A self-recursive binding — a hoisted nested `function` declaration used
// before its textual position, and a self-recursive arrow — is tied with a
// shared cell so the closure can reach itself. That cell carries the BINDING's
// own type: a declaration whose parameters and return are concrete keeps a
// concrete `Rc<dyn Fn(..)>` knot, and nothing about being recursive erases it.

// Hoisted AND self-recursive: `sumDown` is called before it is declared, and
// calls itself.
function totalDown(start: number): number {
  return sumDown(start)
  function sumDown(value: number): number {
    if (value <= 0) {
      return 0
    }
    return value + sumDown(value - 1)
  }
}

// The same knot reached through an arrow: `repeat` captures itself.
function banner(text: string, times: number): string {
  const repeat = (left: number, acc: string): string => {
    if (left <= 0) {
      return acc
    }
    return repeat(left - 1, acc + text)
  }
  return repeat(times, '')
}

// A hoisted declaration that closes over a `const` declared above it and
// recurses on a string, so the knot's type is not a number-shaped one.
function trim(label: string): string {
  const suffix = '!'
  return strip(label)
  function strip(value: string): string {
    if (!value.endsWith(suffix)) {
      return value
    }
    return strip(value.slice(0, value.length - 1))
  }
}

console.log(totalDown(4))
console.log(banner('ab', 3))
console.log(trim('done!!!'))
