// A class instance handed to an `any` slot is the instance itself: a write
// through the erased value (a data field or a function field) is seen by
// every typed handle on it.
class Box {
  f: (x: number) => number = (x) => x + 1;
  v = 1;
}
function patch(o: any) { o.f = (x: number) => x + 100; o.v = 5; }
const b = new Box();
patch(b);
console.log(b.f(1), b.v);
const objs: Box[] = [new Box()];
const z: any = objs[0];
z.f = () => 42;
z.v = 6;
console.log(objs[0].f(0), objs[0].v);
