// `fn.apply(thisArg, list)` spreads the list over a typed callee's parameters.

const zero = (): string => "zero";
const two = (a: number, b: number): number => a + b;
const optional = (a: number, b?: number): string => `${a}:${b}`;
const lead = (a: number, ...rest: number[]): string => `${a}|${rest.join(",")}`;
const host: any = { k: 1 };
const pair: number[] = [4, 5, 6];

// A fixed-arity callee reads its parameters from the list by index; missing
// ones are `undefined`, extra ones are ignored.
console.log(zero.apply(host, []), two.apply(host, [1, 2]), two.apply(host, pair as [number, number]));
console.log(optional.apply(host, [7]));

// A callee with leading parameters before its rest splits the list.
const triple: number[] = [1, 2, 3];
console.log(lead.apply(host, triple as [number, ...number[]]), lead.apply(host, [9]), lead(...(triple as [number, ...number[]])));

// The leading operand is the receiver, not an argument.
function readsThis(this: any, n: number): string {
  return `${this?.k}-${n}`;
}
console.log(readsThis.apply(host, [3]), readsThis.call(host, 4));
