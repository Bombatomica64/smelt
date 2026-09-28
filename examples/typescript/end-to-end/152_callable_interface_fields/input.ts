// Class FIELDS whose declared type is a callable interface: calling the member
// calls the stored value, the same way a field typed as an arrow type is
// called, whatever the interface's call signatures look like.

// A single call signature.
interface Get {
  (key: string): number;
}

// An overload set sharing one result, spelled like Hono's `Context.set`. The
// module's own `Set` shadows the global collection type of the same name.
interface Set {
  (key: string, value: number): void;
  (key: string, value: string): void;
}

// Overloads that differ in arity: omitted positions are optional.
interface SetHeaderOptions {
  append?: boolean;
}

interface SetHeader {
  (name: string): void;
  (name: string, value: string, options?: SetHeaderOptions): void;
}

// A generic call signature: its type parameter is bounded by `string`.
interface Echo {
  <K extends string>(key: K): K;
}

class Context {
  store: Map<string, string> = new Map();
  headers: Map<string, string> = new Map();
  reads = 0;

  get: Get = (key: string) => {
    this.reads += 1;
    return (this.store.get(key) ?? "").length;
  };

  set: Set = (key: string, value: number | string) => {
    this.store.set(key, String(value));
  };

  header: SetHeader = (name: string, value?: string, options?: SetHeaderOptions) => {
    if (value === undefined) {
      this.headers.delete(name);
      return;
    }
    const previous = this.headers.get(name);
    if (options?.append && previous !== undefined) {
      this.headers.set(name, previous + ", " + value);
      return;
    }
    this.headers.set(name, value);
  };

  echo: Echo = <K extends string>(key: K): K => key;

  // Calls through `this` from inside a method.
  touch(): number {
    this.set("inner", 42);
    this.header("x-inner", "yes");
    return this.get("inner");
  }
}

const c = new Context();
c.set("a", "hello");
c.set("b", 7);
console.log(c.get("a"));
console.log(c.get("b"));
console.log(c.touch());
console.log(c.echo("zz"));

c.header("x-one", "1");
c.header("x-one", "2", { append: true });
c.header("x-two", "2");
c.header("x-two");
console.log(c.headers.get("x-one"));
console.log(c.headers.has("x-two"));
console.log(c.headers.get("x-inner"));

// Read as a value, then called: the arrow captured `this` when it was created.
const read = c.get;
console.log(read("a"));
console.log(c.reads);
