// A loop nested in a branch of another loop, and a nested `for` head that
// shadows the outer one's counter.
//
// Two defects met in Hono's trie-router `#pushHandlerSets`, and either alone
// hung it:
//
// * The emitter recognized a `while` header by plain reachability: a block
//   that returns to itself. Every block inside an OUTER loop does, by going
//   around it, so the `if` below was emitted as a spurious `loop {}` and the
//   inner loop's `continue` re-ran its preheader forever. A header now needs a
//   genuine back edge — a path that does not leave through one of its strict
//   dominators.
// * The frontend's name bindings were flat per function, so the inner
//   `for (let i ..)` overwrote the outer `i` and the outer `i++` (lowered after
//   the body) incremented the inner counter. Block and `for`-head `let`/`const`
//   bindings now restore the outer binding they shadowed.
//
// Every line below is diffed against Node.

const out: string[] = [];
for (let i = 0, len = 3; i < len; i++) {
  const row = [10, 20];
  if (i !== 1) {
    for (let i = 0, len = row.length; i < len; i++) {
      out.push(`${row[i]}`);
    }
  }
  out.push(`/${i}`);
}
console.log(out.join(','));

let total = 0;
let k = 0;
while (k < 3) {
  if (k % 2 === 0) {
    let j = 0;
    while (j < k + 1) {
      total += j;
      j++;
    }
  }
  k++;
}
console.log(total);

const x = 'outer';
{
  const x = 'inner';
  console.log(x);
}
console.log(x);

// Three loops deep, the innermost a `for..of` with a `continue`: a nested loop
// past the first level was not recognized as a loop at all, so its `continue`
// resumed the MIDDLE loop without that loop's `j++`.
const seen: string[] = []
const rows = [['a', '*'], ['b']]
for (let r = 0; r < 2; r++) {
  for (let j = 0, rowCount = rows.length; j < rowCount; j++) {
    if (r === 1) {
      seen.push('last')
    }
    for (const p of rows[j]) {
      if (p === '*') {
        seen.push('star')
        continue
      }
      seen.push(p)
    }
  }
}
console.log(seen.join(','))
