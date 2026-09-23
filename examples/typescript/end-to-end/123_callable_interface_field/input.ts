// A class field whose declared type is a callable interface.
//
// `store.set(k, v)` reads the FIELD and calls it through the interface's call
// signature, exactly as for a function-typed field; the interface only
// supplies the signature. The interface below is generic and overloaded, the
// shape of Hono's `Context.set: Set<E>` -- and, like Hono's, it is NAMED `Set`:
// a module-local declaration shadows the library `Set` collection for every
// reference in this module.

interface Vars {
  count: number;
}

interface Set<V> {
  <K extends keyof V>(key: K, value: V[K]): void;
  (key: string, value: number): void;
}

interface Getter {
  (key: string): number;
}

class Store<V> {
  values: Map<string, number> = new Map();
  set: Set<V> = (key: string, value: number) => {
    this.values.set(key, value);
  };
  get: Getter = (key: string) => {
    return this.values.get(key) ?? 0;
  };
}

const store = new Store<Vars>();
store.set("count", 1);
store.set("b", 2);
console.log(store.get("count") + store.get("b"));

const local: Getter = (key: string) => key.length;
console.log(local("four"));
