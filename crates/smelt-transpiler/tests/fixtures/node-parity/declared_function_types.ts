// A binding's declared function type is what a call through it infers from, on
// every way of declaring one, and whichever way the generic callee is reached.
// module-level const with declared type
const mh: (acc: unknown, idx: number) => unknown = (acc, idx) => `${acc}-${idx}`
function iter<T>(count: number, func: (v: T, i: number) => T, init: T): T {
  let value = init
  for (let i = 1; i <= count; i++) { value = func(value, i) }
  return value
}
console.log(iter(2, mh, 0))
// method generic
class C {
  it<T>(n: number, f: (v: T, i: number) => T, init: T): T { let v = init; for (let i = 0; i < n; i++) v = f(v, i); return v }
}
console.log(new C().it(2, mh, 0))
// stdlib
const red = [1, 2].reduce(mh, 0)
console.log(red)
const r2: unknown[] = [1, 2].map(x => mh(x, 1))
console.log(r2.join(","))
// local generic closure
function loc(): unknown {
  const it = <T>(f: (v: T) => T, v: T): T => f(v)
  const hh: (v: unknown) => unknown = v => `${v}L`
  return it(hh, 0)
}
console.log(loc())
// overload
function ov<T>(f: (v: T) => T, v: T): T
function ov<T>(f: (v: T) => T, v: T, n: number): T
function ov<T>(f: (v: T) => T, v: T, n?: number): T { return f(v) }
const oh: (v: unknown) => unknown = v => `${v}O`
console.log(ov(oh, 0))
// annotated function-valued class field / object prop
const obj: { f: (a: unknown) => unknown } = { f: a => `${a}F` }
console.log(iter(1, obj.f, 0))
// through a parameter
function viaParam(cb: (a: unknown, i: number) => unknown) { return iter(1, cb, 0) }
console.log(viaParam((a, i) => `${a}P${i}`))
// type alias
type Fn = (a: unknown, i: number) => unknown
const af: Fn = (a, i) => `${a}A${i}`
console.log(iter(1, af, 0))
async function main() {
  const ah: (a: unknown) => Promise<unknown> = async a => `${a}Z`
  console.log(await ah(1))
}
main()

// class generic
class P<T> { a: T; f: (v: T, i: number) => T; constructor(a: T, f: (v: T, i: number) => T) { this.a = a; this.f = f } run(): T { return this.f(this.a, 1) } }
console.log(new P(0, mh).run())
