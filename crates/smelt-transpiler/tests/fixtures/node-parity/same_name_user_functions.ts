// A user function is called as written whatever its name: no library's
// spelling of a helper (`sliceString`, `partialBind`) changes what it does.
function sliceString(n: number) { return (s: string) => s + "!" + n; }
console.log(sliceString(1)("abc"));
function partialBind(f: (a: number, b: number) => number, a: number) { return (b: number) => f(a, b) * 100; }
console.log(partialBind((x, y) => x - y, 10)(3));
function partialLastBind(f: (a: string, b: string) => string, b: string) { return (a: string) => f(b, a) + "?"; }
console.log(partialLastBind((x, y) => x + y, "L")("R"));
