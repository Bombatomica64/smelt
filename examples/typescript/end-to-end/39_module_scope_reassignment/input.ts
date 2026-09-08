// A module-scope `let` can be reassigned, and the reassignment has to be kept.
// It used to be discarded: any name with a recorded declared type -- which is
// every annotated or literal-initialized module-level binding -- took a
// "module global" path that evaluated the right-hand side for its side effects
// and threw the value away. So `let n: number | undefined; n = 5` still read
// `undefined`, and `let m: number = 0; m = 6` still read `0`, with no
// diagnostic. The same code inside a function was always correct, which is the
// comparison this fixture keeps.
//
// A contextual type also flows through an immediately-invoked function
// expression's return position, so the arrow the IIFE returns gets its
// parameter types from the assignment target rather than erasing.
type Sizer = (value: string) => number;

let annotated: number | undefined;
annotated = 5;

let initialized: number = 1;
initialized = 6;

let viaIife: Sizer | undefined;
viaIife = (() => {
  const offset = 10;
  return (value) => value.length + offset;
})();

let viaOrAssign: Sizer | undefined;
viaOrAssign ||= (() => {
  const offset = 20;
  return (value) => value.length + offset;
})();

let viaNullishAssign: Sizer | undefined;
viaNullishAssign ??= (() => {
  const offset = 30;
  return (value) => value.length + offset;
})();

const direct: Sizer = (() => {
  const offset = 40;
  return (value) => value.length + offset;
})();

function insideAFunction(): string {
  let local: number | undefined;
  local = 7;
  let seeded: number = 2;
  seeded = 8;
  return `${local === undefined ? -1 : local}/${seeded}`;
}

console.log(annotated === undefined ? -1 : annotated);
console.log(initialized);
console.log(viaIife === undefined ? -1 : viaIife('ab'));
console.log(viaOrAssign === undefined ? -1 : viaOrAssign('ab'));
console.log(viaNullishAssign === undefined ? -1 : viaNullishAssign('ab'));
console.log(direct('ab'));
console.log(insideAFunction());
