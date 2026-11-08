// A field called `value` or `done` on a narrowed optional is that field; only an
// iterator result has the iterator protocol.
class Box { value: number; constructor(v: number) { this.value = v } }
function asBox(b: unknown) { const x = b as Box; return x.value + 1 }
class Holder {
  b?: Box
  constructor(b?: Box) { this.b = b }
  read(): number { if (this.b) return this.b.value; return -1 }
}
console.log(asBox(new Box(41)), new Holder(new Box(7)).read(), new Holder().read())

function* g() { yield 1; yield 2 }
const it = g()
const a = it.next()
console.log(a.value, a.done)
const m = new Map<string, number>([["a", 1]])
const mi = m.values()
const s = mi.next()
console.log(s.value, s.done)

function firstIs(xs: Iterator<number>): boolean { const f = xs.next(); return "done" in f }
const mi2 = new Map([["k", 9]]).values()
const st = mi2.next()
console.log(st)
for (const i in [7, 8]) console.log(i)
const arr2 = ["p", "q"]
for (const i in arr2) console.log(i, arr2[i])
const tup: [number, string] = [1, "z"]
for (const i in tup) console.log(i)
class V { value = 1; done = false }
function pv(x?: V): number { if (x) { return x.done ? 0 : x.value } return -1 }
console.log(pv(new V()), pv())
const c: { value: number } | undefined = { value: 3 }
console.log(c?.value)
function* gg() { yield 5 }
const gi = gg()
console.log(gi.next())
console.log(gi.next())
const pe = ["a", "b"].entries()
const fe = pe.next()
if ("done" in fe && fe.done) { console.log("empty") }
const { value: [, firstV] } = fe as IteratorYieldResult<[number, string]>
console.log(firstV)
const mi3 = new Map([["k", 9]]).values()
const { value: vv, done: dd } = mi3.next()
console.log(vv, dd)
function asserted(b: Box | undefined): number { const x = b as Box; return x.value + 1 }
console.log(asserted(new Box(1)))
class Step { done = true; value = "own" }
function step(s?: Step): string { if (s && s.done) return s.value; return "none" }
console.log(step(new Step()), step())
const boxes = new Map([["b", new Box(2)]]).values()
const boxStep = boxes.next()
console.log(boxStep.value?.value, boxStep.done)
