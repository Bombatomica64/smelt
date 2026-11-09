// `typeof` of a value statically known to be undefined.

function nothing(): void {}

const explicit: undefined = undefined;
const fromVoid = nothing();
let unset: undefined;

console.log(typeof explicit);
console.log(typeof fromVoid);
console.log(typeof unset);
console.log(typeof undefined);
console.log(typeof explicit === "undefined");
console.log(typeof explicit === "object");
