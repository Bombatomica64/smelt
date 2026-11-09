const log: string[] = [];
const target: Record<string, number> = { a: 1 };
const p: any = new Proxy(target, {
  get(t: any, k: any) { log.push('get:' + String(k)); return k === 'x' ? 42 : t[k]; },
  set(t: any, k: any, v: any) { log.push('set:' + String(k)); t[k] = v * 2; return true; },
  has(t: any, k: any) { return k === 'magic' || k in t; },
  ownKeys() { return ['a', 'b']; },
});
console.log(p.x, p.a, p.missing);
p.b = 3;
p['c'] = 4;
console.log(target.b, target.c, 'magic' in p, 'nope' in p, 'a' in p);
console.log(Reflect.ownKeys(p));
console.log(log);
const passthrough: any = new Proxy({ v: 1 } as any, { get: (t: any, k: any) => t[k] });
passthrough.w = 2;
console.log(passthrough.v, passthrough.w, Object.keys(passthrough));
const fn: any = new Proxy(function (n: number) { return n + 1; } as any, {
  apply: (t: any, _self: any, args: any[]) => t(...args) * 10,
  get: (t: any, k: any) => (k === 'tag' ? 'tagged' : t[k]),
});
console.log(fn(1), fn.tag);
const noApply: any = new Proxy((n: number) => n * 3, { get: (t: any, k: any) => t[k] });
console.log(noApply(2));
const thrower: any = new Proxy({}, { get() { throw new RangeError('trap'); } });
try { console.log(thrower.x); } catch (e) { console.log((e as Error).message, e instanceof RangeError); }
const empty = new Proxy({ typed: 5 }, {});
console.log(empty.typed);
const FUNC2: any = () => {}
const o2 = { source: 'c', doors: 2 }
const t2: any = Object.assign(FUNC2, o2)
console.log(t.doors, Object.keys(t2))
const c2: any = Object.assign({}, t2)
console.log(c2, c2.doors)
const self2: any = {...c2, id: 'x'}
console.log(self2.doors)
const px2: any = new Proxy(t2, { get: (x: any, k: string) => x[k], apply: (x: any, s: any, a: any[]) => ({ ...Object.assign({}, x), id: a[0] }) })
console.log(px2.doors, px2('23').doors, Object.keys(px2))
const fn3 = (self: any) => (id: string) => ({ ...self, id })
const fn4: any = fn3({ a: 1 })
const args3: any[] = ['23']
console.log(fn4(...args3), fn3({ b: 2 })(...(args3 as [string])))
function callable3<F extends (...a: any) => any>(g: (s: any) => F) { return (args: any[]) => g({ z: 1 })(...args3) }
console.log(callable3(s => (id: string) => ({ ...s, id }))(['9']))
