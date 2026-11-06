// A generator whose `T` is reachable only through a union answers what its definition yields.
function* seq<T = number>(count: number, valueOrMapper: T | ((i: number) => T)): Generator<T> {
  const mapper = typeof valueOrMapper === "function" ? (valueOrMapper as (i: number) => T) : () => valueOrMapper;
  for (let i = 0; i < count; i++) { yield mapper(i); }
}
console.log([...seq(2, "y")], [...seq(2, (i) => i * 2)]);
const collect = (g: Generator<unknown>): unknown[] => [...g];
console.log(collect(seq(1, true)));
