// A member a program declares is what a call finds, whatever the name: the
// builtin of that name is only reached by a receiver that has no such member.
interface Router { match(path: string): string; replace(a: string): string; }
class Impl implements Router {
  match(path: string): string { return 'ok:' + path; }
  replace(a: string): string { return 'rep:' + a; }
}
const routers: Router[] = [new Impl()];
console.log(routers[0].match('p'), routers[0].replace('p'));
const r: Router = new Impl();
console.log(r.match('q'), r.replace('q'));
console.log([r].map((x) => x.match('z')).join(','));
console.log([r].map((x) => x.replace('z')).join(','));
interface Prop { match: (p: string) => string; split: (p: string) => string; }
const ps: Prop[] = [{ match: (p) => 'prop:' + p, split: (p) => 's:' + p }];
console.log(ps[0].match('a'), ps[0].split('b'));
type Lit = { match(p: string): string; search(p: string): number };
const ls: Lit[] = [{ match(p: string) { return 'lit:' + p; }, search(p: string) { return p.length; } }];
console.log(ls[0].match('a'), ls[0].search('bcd'));
class Field { match: (p: string) => string = (p) => 'fld:' + p; }
const fs: Field[] = [new Field()];
console.log(fs[0].match('x'));
function viaBound<T extends Router>(xs: T[]): string { return xs[0].match('g'); }
console.log(viaBound([new Impl()]));
interface Sub extends Router { extra(): number; }
class SubImpl extends Impl { extra(): number { return 1; } }
const subs: Sub[] = [new SubImpl()];
console.log(subs[0].match('s'), subs[0].extra());
function make(): Router { return new Impl(); }
console.log(make().match('m'), (new Impl()).match('n'), (r as Router).match('c'));
const obj = { box: { match(p: string): string { return 'o:' + p; } } };
console.log(obj.box.match('t'));
const strs = ['aXb'];
console.log(strs[0].match(/X/)?.[0], strs[0].replace('X', 'Y'), 'aXb'.split('X').length);
class Names {
  map(f: number): string { return 'map' + f; } filter(f: string): string { return 'filter' + f; } includes(x: string): string { return 'inc' + x; }
  push(a: number): string { return 'push' + a; } slice(a: number): string { return 'slice' + a; } join(s: string): number { return s.length; }
  at(i: number): string { return 'at' + i; } trim(): string { return 'trim'; } startsWith(s: string): string { return 'sw' + s; }
  toFixed(n: number): string { return 'tf' + n; } get(k: string): string { return 'get' + k; } has(k: string): string { return 'has' + k; }
  test(k: string): string { return 'test' + k; } exec(k: string): string { return 'exec' + k; } then(k: string): string { return 'then' + k; }
}
const ns: Names[] = [new Names()];
const n0 = ns[0];
console.log(ns[0].map(1), ns[0].filter('a'), ns[0].includes('b'), ns[0].push(2), ns[0].slice(3), ns[0].join('ab'));
console.log(ns[0].at(1), ns[0].trim(), ns[0].startsWith('z'), ns[0].toFixed(2), ns[0].get('k'), ns[0].has('h'));
console.log(ns[0].test('t'), ns[0].exec('e'), ns[0].then('n'), n0.trim());
