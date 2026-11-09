// `<=` and `>=` hold at equality, for every kind of operand that orders.

function same<T>(a: T, b: T): boolean[] {
  return [(a as unknown as number) <= (b as unknown as number), (a as unknown as number) >= (b as unknown as number)];
}

const numberA: number = 3;
const numberB: number = 3;
console.log(numberA <= numberB, numberA >= numberB, numberA < numberB);

const textA: string = "ab";
const textB: string = "ab";
console.log(textA <= textB, textA >= textB, textA < textB);

const bigA: bigint = 5n;
const bigB: bigint = 5n;
console.log(bigA <= bigB, bigA >= bigB, bigA < bigB);

const dateA = new Date(1000);
const dateB = new Date(1000);
console.log(dateA <= dateB, dateA >= dateB, dateA < dateB);

const erasedA: unknown = 3;
const erasedB: unknown = 3;
console.log((erasedA as number) <= (erasedB as number));

console.log(same(2, 2).join(","));
console.log(1 <= 2, 2 <= 1, "a" <= "b", "b" <= "a");

function erasedLte(a: any, b: any): boolean { return a <= b; }
function erasedGte(a: any, b: any): boolean { return a >= b; }
console.log(erasedLte("m", "m"), erasedGte("m", "m"), erasedLte(4, 4), erasedGte(4, 4));
console.log(erasedLte("a", "b"), erasedLte("b", "a"), erasedLte(1, 2), erasedLte(2, 1));
