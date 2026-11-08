// Operands whose rendered form binds loosely, spliced into postfix and
// operator positions.

function neg(x: number): number {
  return -x;
}
const n = neg(1.5);
console.log("" + -n, "v" + neg(2), `${-n}`);
console.log((-n).toFixed(1), (n * -1).toString());
const s = "ab";
console.log(s + -1, s + (1 + 2));
const t: [number, number] = [-1, 2];
console.log(t[0] === -1 && t[1] === 2);
