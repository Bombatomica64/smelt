// A namespace member call answers what a direct call answers.
import * as _ from "./lib.ts"
const h: (acc: unknown, idx: number) => unknown = (acc, idx) => `${acc}-${idx}`
console.log(_.iterate(2, h, 0))
const h2: (acc: unknown, idx: number) => number = (acc, idx) => (acc as number) + idx
const n = _.iterate(3, h2, 1)
console.log(n + 1)
console.log(_.iterate(2, (a: string) => a + "x", ""))
function sn(): string | number { return "w" }
console.log(_.pick(1, sn()))
