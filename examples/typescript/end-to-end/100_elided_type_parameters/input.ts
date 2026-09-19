// A generic class's Rust arity is the parameters its emitted Rust spells —
// round 32, Agent G item 1.
//
// A hand-writing Rust team does not declare a struct parameter its data never
// holds. A TypeScript type parameter that reaches no position the generated
// Rust renders is type-level plumbing: the frontend keeps it, so type-checking
// is unchanged, and only the Rust arity drops it.
//
// The rule is the LEAST fixpoint of "position `i` of `N` is carried when the
// parameter occurs somewhere `N`'s own emitted Rust spells it", where an
// occurrence nested in another generated type `M<.., a_j, ..>` counts only when
// position `j` of `M` is itself carried. Starting from "nothing is carried" is
// what makes a self-referential position drop, and it is why no erasure is
// needed: an elided parameter is spelled nowhere.
//
// Four shapes, one rule:
//
//   * `Route<Method>` mentions `Method` nowhere at all — pure plumbing.
//   * `Session<Env>` mentions `Env` only inside `Session<Env>` itself, so the
//     least fixpoint never forces it (a greatest fixpoint would keep it).
//   * `Pair<Carried, Dropped>` reaches a `Cell` position that IS carried and a
//     `Route` position that is NOT, so it keeps one parameter and drops one.
//   * `Cell<T>` stores a `T`, so `T` survives — the rule is a fixpoint, not a
//     blanket drop.
//
// Every line below is diffed against Node.

// `Method` reaches nothing: it exists to distinguish `Route<"get">` from
// `Route<"post">` at the type level and never touches a value.
class Route<Method> {
  path: string;
  constructor(path: string) {
    this.path = path;
  }
  describe(): string {
    return "route " + this.path;
  }
}

// `Env` is mentioned exactly once, in a field whose type names the class itself
// at `Env`'s own position. Nothing outside that circle holds an `Env`.
class Session<Env> {
  name: string;
  renew: ((session: Session<Env>) => string) | null;
  constructor(name: string) {
    this.name = name;
    this.renew = null;
  }
  label(): string {
    return "session " + this.name;
  }
}

// `T` is the type of stored data: it survives.
class Cell<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
  get(): T {
    return this.value;
  }
}

// Transitivity in both directions at once. The constructor takes the raw
// pieces rather than the already-built records: passing a `Cell<Carried>` in
// crosses the substituted-record seam that round 31 recorded as open ("FOUND
// AND NOT FIXED — constructing an interface record at a substituted ABI"), and
// that seam is not what this fixture is about.
class Pair<Carried, Dropped> {
  left: Cell<Carried>;
  right: Route<Dropped>;
  constructor(value: Carried, path: string) {
    this.left = new Cell<Carried>(value);
    this.right = new Route<Dropped>(path);
  }
  carried(): Carried {
    return this.left.get();
  }
}

const route = new Route<"get">("/users");
console.log(route.describe());

const session = new Session<number>("s1");
session.renew = (other: Session<number>) => "renewed " + other.name;
console.log(session.label());
console.log(session.renew(session));

const cell = new Cell<string>("held");
console.log(cell.get());

const pair = new Pair<number, "post">(41, "/posts");
console.log(pair.carried() + 1);
console.log(pair.right.describe());
