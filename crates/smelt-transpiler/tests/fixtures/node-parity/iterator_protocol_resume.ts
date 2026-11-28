// Hand-written iterators receive what the consumer sends: `next(value)`,
// `return(value)` and `throw(error)` reach the iterator's own methods, `for...of`
// calls `next()` and `return()` with no argument, and `yield*` forwards each.
class Echo implements Iterator<string, number, number | undefined> {
  private count = 0;
  constructor(private label: string) {}
  next(sent?: number): IteratorResult<string, number> {
    console.log(this.label, "next got", sent);
    this.count++;
    if (this.count > 3) return { value: this.count * 10, done: true };
    return { value: this.label + this.count, done: false };
  }
  return(value?: number): IteratorResult<string, number> {
    console.log(this.label, "return got", value);
    return { value: value ?? -1, done: true };
  }
  throw(error?: unknown): IteratorResult<string, number> {
    console.log(this.label, "throw got", (error as Error).message);
    return { value: "recovered", done: false };
  }
}
class EchoBox implements Iterable<string, number, number | undefined> {
  constructor(private label: string) {}
  [Symbol.iterator](): Iterator<string, number, number | undefined> {
    return new Echo(this.label);
  }
}

// A plain iterator without `throw`.
class Quiet implements Iterator<number, string, number | undefined> {
  private n = 0;
  next(sent?: number): IteratorResult<number, string> {
    console.log("quiet next got", sent);
    return this.n < 5 ? { value: this.n++, done: false } : { value: "end", done: true };
  }
  return(value?: string): IteratorResult<number, string> {
    console.log("quiet return got", value);
    return { value: value ?? "closed", done: true };
  }
}
class QuietBox implements Iterable<number, string, number | undefined> {
  [Symbol.iterator](): Iterator<number, string, number | undefined> {
    return new Quiet();
  }
}

function direct(): void {
  const it: Iterator<string, number, number | undefined> = new Echo("d");
  console.log(it.next(1));
  console.log(it.next());
  console.log(it.next(2));
  console.log(it.return!(7));
  console.log(it.next(3));
  console.log(it.next(4));
  const t: Iterator<string, number, number | undefined> = new Echo("t");
  console.log(t.throw!(new Error("bad")));
  console.log(t.return!());
}

function loops(): void {
  for (const v of new EchoBox("f")) console.log("for", v);
  for (const v of new EchoBox("b")) {
    console.log("break at", v);
    break;
  }
  const [x] = new EchoBox("x");
  console.log("destructured", x);
}

function* outer(): Generator<string, number, number> {
  try {
    const r = yield* new EchoBox("y");
    console.log("inner returned", r);
    return r + 1;
  } finally {
    console.log("outer finally");
  }
}

function delegation(): void {
  const g = outer();
  console.log(g.next(100));
  console.log(g.next(5));
  console.log(g.next(6));
  console.log(g.next(7));
  console.log(g.next(8));
  const h = outer();
  console.log(h.next());
  console.log(h.throw(new Error("thrown in")));
  console.log(h.return(99));
  console.log(h.next());
}

function* quietOuter(): Generator<number, string, number> {
  const r = yield* new QuietBox();
  return "q " + r;
}

function missingThrow(): void {
  const q = quietOuter();
  console.log(q.next());
  console.log(q.next(9));
  try {
    q.throw(new Error("no throw method"));
  } catch (e) {
    console.log(e instanceof TypeError, (e as Error).message);
  }
  console.log(q.next().done);
  const all = quietOuter();
  let step = all.next();
  while (!step.done) step = all.next(step.value * 2);
  console.log("final", step.value);
}

function* inner(): Generator<number, number, number> {
  try {
    const a = yield 1;
    console.log("inner got", a);
    yield 2;
    return 3;
  } finally {
    console.log("inner finally");
  }
}
function* wrapper(): Generator<number, number, number> {
  try {
    const r = yield* inner();
    console.log("after delegation", r);
    return r + 1;
  } finally {
    console.log("wrapper finally");
  }
}
function generatorReturn(): void {
  const w = wrapper();
  console.log(w.next());
  console.log(w.next(11));
  console.log(w.return(50));
  console.log(w.next());
}

direct();
loops();
delegation();
missingThrow();
generatorReturn();
