// bigint operators, its runtime tag behind `unknown`, its 64-bit views and the
// catchable errors of division, shift and JSON serialization.

function mix(a: bigint, b: bigint): bigint {
  return ((a * b) >> 3n) ^ -a;
}
function half(a: bigint): bigint {
  return a / 2n;
}
function below(a: bigint, b: number): boolean {
  return a < b;
}
console.log(String(mix(123456789012345678901234567890n, 987654321n)));
console.log(String(half(-7n)), String(half(1n)), below(2n, 2.5), below(3n, 2.5));

function describe(value: unknown): string {
  return typeof value === "bigint" ? "bigint" : "other";
}
console.log(describe(9_999_999_999_999_999_999_999n), describe(1), describe("1n"));

function roundTrip(): bigint {
  const view = new BigInt64Array([1n, 2n]);
  view[1] = 9_223_372_036_854_775_807n;
  return view[0] + view[1];
}
console.log(String(roundTrip()));

function divide(a: bigint, b: bigint): string {
  try {
    return String(a / b);
  } catch (error) {
    return (error as Error).name;
  }
}
console.log(divide(4n, 2n), divide(1n, 0n));

interface Ledger {
  owner: string;
  balance: bigint;
}
function ledgerJson(ledger: Ledger): string {
  try {
    return JSON.stringify(ledger);
  } catch (error) {
    return (error as Error).name + ": " + (error as Error).message;
  }
}
function plainJson(owner: string): string {
  return JSON.stringify({ owner });
}
console.log(ledgerJson({ owner: "a", balance: 1n }));
console.log(plainJson("b"));

console.log((10n).toString(2), String(1n << 8n), String((1n << 8n) >> 1n), String(1n << 40n));
console.log((4 + 10) >>> 1, 1 << 31, -8 >> 1, 1 << 32, -1 >>> 0);
function badRadix(value: bigint): string {
  try {
    return value.toString(99);
  } catch (error) {
    return (error as Error).name;
  }
}
console.log(badRadix(10n));
