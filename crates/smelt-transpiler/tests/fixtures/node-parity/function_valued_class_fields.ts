// A class field initialized to a function is a function-valued slot: static
// fields live in module or per-evaluation storage, instance fields on each
// instance. Calls through the slot run the stored function, and its `name`
// is the property key (NamedEvaluation), its `length` the declared arity.
class K {
  static base = 10;
  static sfield = function () {};
  static sarrow = () => {};
  static add = (x: number, y: number) => K.base + x + y;
  static own = function named(a: number) { return a; };
  ifield = function (x: number) { return x * 2; };
  iarrow = () => 1;
}
console.log(K.sfield.name, K.sfield.length);
console.log(K.sarrow.name, K.sarrow.length);
console.log(K.add.name, K.add.length, K.add(1, 2));
console.log(K.own.name, K.own.length, K.own(7));
const add = K.add;
console.log(add.name, add(3, 4));
const k = new K();
console.log(k.ifield.name, k.ifield.length, k.ifield(21));
console.log(k.iarrow.name, k.iarrow.length, k.iarrow());
function make(n: number) {
  class L {
    static n = n;
    static twice = () => L.n * 2;
  }
  return L;
}
const L3 = make(3);
console.log(L3.twice.name, L3.twice.length, L3.twice(), make(5).twice());
