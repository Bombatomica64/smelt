// `Math.max`/`Math.min` answer `NaN` when any argument is `NaN`, order `-0`
// below `+0`, and are `-Infinity`/`Infinity` with no arguments.
const xs = [3, 1, 2];
console.log(Math.max(...xs), Math.min(...xs), Math.max(), Math.min());
console.log(Math.max(1, NaN), Math.max(NaN, 1), Math.min(...[3, NaN]), Math.min(1, NaN), Math.max(...[NaN, 1]));
console.log(Object.is(Math.max(-0, 0), 0), Object.is(Math.max(0, -0), 0), Object.is(Math.min(0, -0), -0), Object.is(Math.min(-0, 0), -0));
console.log(Math.max(1, 2, 3), Math.min(4, -Infinity, 2), Math.max(0, ...xs, 10));
