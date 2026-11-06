// An operation on a narrowed union reads the value it narrowed to, never a default.
class Box { value: number; constructor(v: number) { this.value = v } }
function trim(str: string, chars?: string | string[]): string {
  if (chars === undefined) return str.trim()
  let out = str
  if (Array.isArray(chars)) {
    const flat = chars.flatMap(c => c.split(''))
    return flat.join('-')
  }
  return out + chars
}
console.log(trim("x", ["a", "bc"]))
console.log(trim("x", "ab"))
console.log(trim(" x "))
function g(x: string | number[]): number {
  if (Array.isArray(x)) return x.map(v => v + 1).reduce((a, b) => a + b, 0)
  return x.length
}
console.log(g([1, 2]), g("abc"))
function h(x: string | string[]): string[] {
  if (Array.isArray(x)) return x.filter(s => s.length > 0).concat(x.slice(0, 1))
  return [x]
}
console.log(h(["p", ""]).join(), h("q").join())

function a1(x?: unknown): number {
  if (Array.isArray(x)) return x.flatMap(v => [v, v]).length
  return -1
}
console.log(a1([1, 2]), a1(3), a1())
function a2(x: unknown): number { return (x as Set<number>).size }
console.log(a2(new Set([1, 2])))
function a4(x: unknown): number { const m = x as Map<string, number>; return m.size }
console.log(a4(new Map([["a", 1]])))
function a5(x: string | string[] | undefined): number {
  if (x === undefined) return 0
  if (typeof x === "string") return 1
  return x.map(s => s.length).length + 10
}
console.log(a5(["a"]), a5("a"), a5(undefined))
function a6(x?: [number, string] | number): string {
  if (Array.isArray(x)) return x[1]
  return "n"
}
console.log(a6([1, "t"]), a6(2))
function a7(x?: Box | string): number {
  if (x instanceof Box) return x.value + 1
  return 0
}
console.log(a7(new Box(1)), a7("s"), a7())
function a8(x: unknown): number { const b = x as Box; return b.value }
console.log(a8(new Box(4)))
function a9(x?: Record<string, number> | string): number {
  if (typeof x === "object") return Object.keys(x).length
  return -1
}
console.log(a9({ a: 1 }), a9("q"))

console.log([1, 2].flat().join())
console.log([[1], [2, 3]].flat().join())
const r: Record<string, number> = { "1": 5 }
console.log(r[1])
const m = new Map<number, string>([[1, "a"]])
console.log(m.get(1))
const t: [number, string] = [1, "x"]
console.log(t.length, t[1])
