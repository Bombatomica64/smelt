// A nested `function` declaration is bound for the WHOLE enclosing scope, so a
// statement that textually precedes it may call it, and its own name is in
// scope inside its own body.

// Used before it is declared: the declaration is lowered just before the first
// statement that mentions it, which is also what keeps the `const` it closes
// over in scope at the point the closure is built.
function scaled(base: number): number {
  const scale = 3
  return step(base)
  function step(value: number): number {
    return value * scale
  }
}

// Declared before it is used, and recursive through its own name.
function countUp(base: number): number {
  const limit = 3
  function walk(i: number): number {
    if (i >= limit) {
      return i
    }
    return walk(i + 1)
  }
  return walk(base)
}

// Both at once: hoisted AND self-recursive.
function countUpHoisted(base: number): number {
  const limit = 4
  return climb(base)
  function climb(i: number): number {
    if (i >= limit) {
      return i
    }
    return climb(i + 1)
  }
}

// A declaration in a nested block hoists within THAT block only.
function pickLabel(flag: boolean): string {
  if (flag) {
    return decorate('on')
    function decorate(text: string): string {
      return '[' + text + ']'
    }
  }
  return 'off'
}

console.log(scaled(2))
console.log(countUp(0))
console.log(countUpHoisted(1))
console.log(pickLabel(true))
console.log(pickLabel(false))
