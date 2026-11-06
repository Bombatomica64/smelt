// Every operand keeps the value it had where it was evaluated, whatever a later
// sibling writes: a closure, an update expression, a field store.

function pair(first: number, second: number): number { return first * 10 + second; }
function bumpers(): void {
  let x = 1;
  const bump = (): number => { x = x + 1; return x; };
  console.log(pair(x, bump()));
  console.log([x, bump()].join(","));
  console.log(x + bump());
  console.log(`${x}-${bump()}`);
  const o = { p: x, q: bump() };
  console.log(o.p, o.q);
  const read = (): number => pair(x, bump());
  console.log(read());
  const before = x;
  bump();
  console.log(before, x);
}
function updates(): void {
  let a = 1;
  console.log(pair(a, a++), a);
  console.log(pair(a++, a), a);
  console.log(pair(++a, a), a);
  console.log(pair(a--, a--), a);
  const xs = [a, a++, a];
  console.log(xs.join(","));
  console.log(a + a++ + a);
  let i = 0;
  const arr = [10, 20, 30];
  console.log(arr[i++], arr[i++], i);
  arr[i++] = i;
  console.log(arr.join(","), i);
  let y = 0;
  y = i++;
  console.log(y, i);
  let w = 0;
  while (w++ < 3) { if (w === 2) { console.log("two"); } }
  console.log(w);
  for (let k = 0, j = 5; k < j; k++, j--) { console.log(k, j); }
}
function members(): void {
  const c = { n: 1 };
  const inc = (): number => { c.n = c.n + 1; return c.n; };
  console.log(pair(c.n, inc()));
  console.log(pair(c.n, c.n++));
  let s = "a";
  const add = (): string => { s = s + "b"; return s; };
  console.log(s + add());
  console.log([s, add()].join("|"));
}
function lists(): void {
  let xs = [1];
  const reset = (): number => { xs = [7, 8]; return xs.length; };
  xs.push(2);
  console.log(xs.join(","));
  console.log(xs.length, reset(), xs.length);
  xs.push(9);
  console.log(xs.join(","));
}
function closureLoop(): void {
  let total = 0;
  const add = (n: number): number => { total = total + n; return total; };
  for (let i = 1; i <= 3; i++) { console.log(pair(total, add(i))); }
}
function builtins(): void {
  const xs = [1, 2, 3];
  console.log(pair(xs[0], xs.shift() ?? 0), xs.length);
  const c = { n: 1 };
  const inc = (): number => { c.n = c.n + 1; return c.n; };
  console.log(pair(c.n, [1].map(() => inc())[0]));
}
function armOnly(take: boolean): void {
  let k = 0;
  const xs = [5, 6];
  console.log(take ? xs[k++] : -1, k);
}
armOnly(true);
armOnly(false);
bumpers(); updates(); members(); lists(); closureLoop(); builtins();
