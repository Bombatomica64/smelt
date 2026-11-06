// A generic call instantiates T from every candidate position: a callback return,
// a later value argument, or explicit type arguments, not only the first value.
const iterate = <T>(count: number, func: (v: T, i: number) => T, init: T) => {
  let value = init
  for (let i = 1; i <= count; i++) { value = func(value, i) }
  return value
}
function pick<T>(a: T, b: T, first: boolean): T { return first ? a : b }
function wrap<T>(a: T, f: (v: T) => T): T[] { return [a, f(a)] }
function mk<T>(f: (x: T) => void, v: unknown): T[] { f(v as T); return [v as T] }

// callback returning unknown, T unknown
const h1: (acc: unknown, idx: number) => unknown = (acc, idx) => `${acc}-${idx}`
console.log(iterate(2, h1, 0))
// inline annotated arrow
console.log(iterate(2, (acc: unknown, i: number): unknown => `${acc}/${i}`, 0))
// let binding
let h2: (acc: unknown, idx: number) => unknown = (acc, idx) => `${acc}+${idx}`
console.log(iterate(2, h2, 0))
// union candidate from callback
const h3 = (acc: string | number, i: number): string | number => `${acc}~${i}`
console.log(iterate(2, h3, 0))
// value positions
function un(): unknown { return "s" }
function sn(): string | number { return "s2" }
console.log(pick(1, un(), false))
console.log(pick(1, sn(), false))
console.log(pick(sn(), 1, true))
// concrete stays concrete
const h4: (acc: unknown, idx: number) => number = (acc, idx) => (acc as number) + idx
const conc = iterate(5, h4, 0)
console.log(conc + 1)
// nested in a list
const w = wrap<unknown>(0, v => `${v}!`)
console.log(w.length, w[1])
const h5 = (v: unknown): unknown => `${v}?`
const w2 = wrap(0, h5)
console.log(w2.length, w2[1])
// explicit
console.log(iterate<unknown>(2, (a, i) => `${a}^${i}`, 0))
console.log(iterate<string | number>(2, (a, i) => `${a}^${i}`, 0))
// contra only
const m = mk((x: string) => { console.log(x.length) }, "abc")
console.log(m[0].toUpperCase())

function erasedSum(): unknown {
  const handler: (acc: unknown, idx: number) => unknown = (acc, idx) => (acc as number) + idx
  return iterate(5, handler, 0)
}
function strSum(): unknown {
  const handler: (acc: unknown, idx: number) => unknown = (acc, idx) => `${acc}-${idx}`
  return iterate(2, handler, 0)
}
console.log(erasedSum(), strSum())
