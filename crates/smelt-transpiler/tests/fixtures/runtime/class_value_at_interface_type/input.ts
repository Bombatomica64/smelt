// A class instance assigned to an interface type is converted member-wise.
//
// An interface with methods is emitted as a record of callable slots, a class
// as a nominal struct whose methods are not fields. Structural assignability
// (`const r: Router = new ListRouter()`) is about MEMBERS, so each data field
// is copied and each method slot is filled with the class's method bound to
// the instance — which is still the one shared instance, so calls through the
// interface mutate it. The same conversion runs at a callback's return and for
// a generic interface, which is Hono's `newRouter: () => new RegExpRouter()`.
//
// Every line below is diffed against Node.

interface Router<T> {
  name: string;
  add(method: string, handler: T): void;
  count(): number;
}

class ListRouter<T> implements Router<T> {
  name = 'ListRouter';
  items: T[] = [];
  add(method: string, handler: T): void {
    this.items.push(handler);
  }
  count(): number {
    return this.items.length;
  }
}

const direct = new ListRouter<string>();
const viewed: Router<string> = direct;
viewed.add('GET', 'a');
viewed.add('GET', 'b');
console.log(viewed.name, viewed.count(), direct.count());

const runTest = ({ newRouter }: { newRouter: <T>() => Router<T> }) => {
  const router: Router<string> = newRouter();
  router.add('GET', 'x');
  console.log(router.name, router.count());
};
runTest({ newRouter: () => new ListRouter() });
