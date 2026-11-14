// A generic alias over a union with an object arm (`{ value: V; rest: string }
// | null`) keeps its shape: the arm is a generic record, the alias an optional
// of it, and a generic helper that maps it stays generic. `if (!result) return
// result;` returns the narrowed null, which TypeScript accepts for any `B`.
type ParseResult<V> = { value: V; rest: string } | null;

function mapValue<A, B>(result: ParseResult<A>, mapFn: (value: A) => B): ParseResult<B> {
  if (!result) return result;
  return { value: mapFn(result.value), rest: result.rest };
}

function digits(s: string): ParseResult<number> {
  const m = /^\d+/.exec(s);
  return m ? { value: Number(m[0]), rest: s.slice(m[0].length) } : null;
}

function word(s: string): ParseResult<string> {
  const m = /^[a-z]+/.exec(s);
  if (!m) return null;
  return { value: m[0], rest: s.slice(m[0].length) };
}

function wrap<V>(value: V, rest: string): ParseResult<V> {
  return { value, rest };
}

<<<<<<< HEAD
=======
// `flatMapAll([], (n) => [[n]])` instantiates `T` at `never`: the callback is
// never called, and its parameter keeps the callee's signature.
function flatMapAll<T, U>(xs: T[], f: (x: T) => U[][]): U[] {
  const out: U[] = [];
  for (const x of xs) {
    for (const inner of f(x)) {
      for (const item of inner) out.push(item);
    }
  }
  return out;
}

>>>>>>> origin/t3/corpus-generic-records-main
function main(): void {
  const twice = mapValue(digits("21x"), (n) => n * 2);
  console.log(twice ? twice.value + 1 : -1, twice ? twice.rest : "none");
  const label = mapValue(digits("7"), (n) => ({ n, big: n > 5 }));
  console.log(label ? `${label.value.n} ${label.value.big} [${label.rest}]` : "none");
  const missing = mapValue(digits("x"), (n) => n + 1);
  console.log(missing === null, missing ? missing.value : "absent");
  const shout = mapValue(word("abc12"), (w) => w.toUpperCase());
  console.log(shout ? shout.value + shout.rest : "none");
  const nested = mapValue(wrap(wrap(3, "in"), "out"), (inner) => (inner ? inner.value * 10 : 0));
  console.log(nested ? nested.value : -1, nested ? nested.rest : "");
<<<<<<< HEAD
=======
  console.log(JSON.stringify(flatMapAll([], (n) => [[n]])), JSON.stringify(flatMapAll([1, 2], (n) => [[n, n * 10]])));
>>>>>>> origin/t3/corpus-generic-records-main
}

main();
