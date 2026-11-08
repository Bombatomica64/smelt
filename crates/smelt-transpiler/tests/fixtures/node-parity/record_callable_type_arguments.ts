// A generic record whose callable fields mention its type argument, rebuilt
// from the erased instantiation a generic function returns (ts-pattern's
// `Match<i>`), keeps working callables: the nested callback is wrapped back
// to the erased parameter, and a callback passed to the slot meets the
// slot's declared ABI.
interface Match<i> {
  otherwise: (handler: (value: i) => unknown) => unknown;
  run: () => unknown;
}

function match<i>(value: i): Match<i> {
  return {
    otherwise: (handler: (value: unknown) => unknown) => handler(value),
    run: () => value,
  } as any;
}

const xs: number[] = [1, 2, 3];
const listMatch: Match<number[]> = match(xs);
console.log(listMatch.otherwise((v) => v.length));
console.log(listMatch.run());

const numberMatch = match(5);
console.log(numberMatch.otherwise((v) => v + 1));

type Shape = "circle" | "square";
const shape: Shape = "square";
const shapeMatch: Match<Shape> = match(shape);
console.log(shapeMatch.otherwise((s) => (s === "square" ? 4 : 0)));
