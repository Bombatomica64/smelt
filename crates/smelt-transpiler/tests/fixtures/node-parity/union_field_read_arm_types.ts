// A field every union arm declares at a different type reads as the union of
// those types: each arm's value, typed, in one block or in sibling blocks.
interface N { kind: 'n'; value: number }
interface S { kind: 's'; value: string }
type In = N | S;
function k(input: In) { const v = input.value; console.log(v, typeof v); return v; }
k({ kind: 'n', value: 2 }); k({ kind: 's', value: 'abc' });
function g(input: In) {
  if (typeof input.value === 'number') { const v = input.value; console.log(v + 1); }
  else { const v = input.value; console.log(v.length); }
}
g({ kind: 'n', value: 2 }); g({ kind: 's', value: 'abc' });
type Anon = { t: 'a'; x: number } | { t: 'b'; x: boolean };
const ys: Anon[] = [{ t: 'a', x: 1 }, { t: 'b', x: false }];
for (const y of ys) { const x = y.x; console.log(String(x)); }
