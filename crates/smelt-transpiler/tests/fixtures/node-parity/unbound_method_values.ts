// A prototype method read off a value takes its receiver from the CALL.

function attempt(label: string, run: () => unknown): void {
  try {
    console.log(label, run());
  } catch (error) {
    console.log(label, (error as Error).name, (error as Error).message);
  }
}

const n: any = 3.14159;
const s: any = "hello";
const a: any = [1, 2, 3];

// Detached and called without a receiver: Node's TypeError.
attempt("number", () => { const f = n.toFixed; return f(1); });
attempt("string", () => { const f = s.slice; return f(1); });
attempt("array", () => { const m = a.map; return m((x: number) => x * 2); });
attempt("valueOf", () => { const v = n.valueOf; return v(); });

// An explicit receiver works.
attempt("call", () => { const f = n.toFixed; return f.call(n, 2); });
attempt("string call", () => { const f = s.slice; return f.call(s, 1, 3); });
attempt("array call", () => { const m = a.map; return m.call(a, (x: number) => x + 1); });
attempt("apply", () => { const j = a.join; return j.apply(a, ["-"]); });
attempt("bind", () => { const f = n.toFixed; const g = f.bind(n); return g(3); });
attempt("bind args", () => { const sl = s.slice; const tail = sl.bind(s, 2); return tail(); });

// Member calls, in and out of closures, still supply the receiver.
attempt("direct", () => [n.toFixed(2), s.slice(1), a.map((x: number) => x * 3)]);
const later = () => a.filter((x: number) => x > 1);
attempt("closure", () => later());

// One function per method, with Node's `length`.
const b: any = [4, 5];
attempt("identity", () => [a.map === b.map, a.map === a.filter, s.slice === "x".slice]);
attempt("length", () => [a.map.length, a.slice.length, a.pop.length, s.slice.length, s.trim.length, n.toFixed.length]);

// The receiver is any value the method accepts.
attempt("array-like", () => { const m = a.map; return m.call("abc", (c: string) => c.toUpperCase()); });
attempt("string coerced", () => { const f = s.slice; return f.call(12345, 1, 3); });
attempt("number wrong this", () => { const f = n.toFixed; return f.call("1", 1); });

// A detached call inside a method call does not see that call's receiver.
const holder: any = {
  run() {
    const f = s.slice;
    return f(1);
  },
};
attempt("nested", () => holder.run());

// Typed receivers read the same unbound methods.
const tn: number = 2.5;
const ts: string = "typed";
const ta: number[] = [7, 8];
attempt("typed number", () => { const f = tn.toFixed; return f(0); });
attempt("typed string", () => { const f = ts.slice; return f(1); });
attempt("typed array", () => { const m = ta.map; return m((x: number) => x); });
attempt("typed call", () => { const f = tn.toFixed; return [f.call(tn, 0), ts.slice.call(ts, 2), ta.map.call(ta, (x: number) => x * 10)]; });
attempt("typed identity", () => [ts.slice === s.slice, ta.map === a.map]);
