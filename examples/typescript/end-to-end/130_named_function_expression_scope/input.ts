// A named function expression binds its inner name inside its own body only.
type Handler = (label: string) => string;

function walk(n: number): string {
  return "outer walk " + n;
}

// The inner `walk` is the expression itself (recursion), shadowing the
// module-level `walk` only inside this body.
const depth = function walk(n: number): number {
  return n <= 0 ? 0 : 1 + walk(n - 1);
};

export const factorial = function fact(n: number): number {
  return n <= 1 ? 1 : n * fact(n - 1);
};

// The inner name matches the enclosing const, as in `requestId = () =>
// function requestId(..) {..}`: it must not declare a second item.
export const tagger = (prefix: string): Handler => {
  return function tagger(label) {
    return prefix + label;
  };
};

function report(): void {
  console.log(depth(4));
  console.log(factorial(5));
  console.log(walk(7));
}

report();
console.log(depth(2));
console.log(tagger("id-")("x"));
