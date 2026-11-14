// Shapes that contain themselves by value: mutual and self recursion through
// optional fields, union arms, a generic node, a by-value generic wrapper, and
// a class. Each must compile (no E0072) and keep JavaScript's sharing.
interface L { name: string; o?: O }
interface O { id: number; l?: L }
interface Tree { value: number; kids: Tree[]; parent?: Tree }
interface Num { kind: "num"; n: number }
interface Add { kind: "add"; l: Expr; r: Expr }
type Expr = Num | Add;
function evaluate(e: Expr): number { return e.kind === "num" ? e.n : evaluate(e.l) + evaluate(e.r); }
interface LinkedNode<T> { value: T; next?: LinkedNode<T> }
function length<T>(node?: LinkedNode<T>): number { let count = 0; let cur = node; while (cur) { count++; cur = cur.next; } return count; }
interface Wrap<T> { inner: T }
interface Doll { size: number; nested?: Wrap<Doll> }
class Chain { constructor(public label: string, public prev?: Chain) {} depth(): number { return this.prev ? 1 + this.prev.depth() : 0; } }
interface P { tag: string; q?: Q }
interface Q { n: number; p?: P }
interface Holder<T> { items: T[] }
interface Forest { trees: Holder<Forest>; name: string }
function main(): void {
  const pq: P = { tag: "p", q: { n: 1, p: { tag: "inner" } } };
  console.log(pq);
  console.log(pq.q?.p?.tag, JSON.stringify(pq));
  const forest: Forest = { name: "f", trees: { items: [{ name: "g", trees: { items: [] } }] } };
  console.log(forest.trees.items[0].name, forest.trees.items.length);
  const l: L = { name: "a", o: { id: 1, l: { name: "b" } } };
  console.log(l.o?.id, l.o?.l?.name, l.o?.l?.o === undefined);
  const root: Tree = { value: 1, kids: [] };
  const child: Tree = { value: 2, kids: [], parent: root };
  root.kids.push(child);
  console.log(root.kids[0].value, child.parent?.value, child.parent === root);
  const sum: Expr = { kind: "add", l: { kind: "num", n: 2 }, r: { kind: "add", l: { kind: "num", n: 3 }, r: { kind: "num", n: 4 } } };
  console.log(evaluate(sum));
  const list: LinkedNode<string> = { value: "x", next: { value: "y", next: { value: "z" } } };
  console.log(length(list), list.next?.next?.value);
  const doll: Doll = { size: 3, nested: { inner: { size: 2 } } };
  console.log(doll.nested?.inner.size, doll.nested?.inner.nested === undefined);
  const c3 = new Chain("c", new Chain("b", new Chain("a")));
  console.log(c3.depth(), c3.prev?.prev?.label);
  const alias = l.o;
  if (alias) { alias.id = 42; }
  console.log(l.o?.id);
  console.log(JSON.stringify(sum));
  console.log(JSON.stringify(list));
}
main();
