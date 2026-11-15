// `new Date(y, m, d, h, min, s, ms)` and `Date.UTC` follow MakeDay/MakeTime:
// out-of-range parts carry into the next unit and a year in 0..99 is 19xx
// (date-fns differenceInMinutes: `new Date(2014, 6, 2, 12, 20, 60)`).
const show = (date: Date): string =>
  [date.getFullYear(), date.getMonth(), date.getDate(), date.getHours(), date.getMinutes(), date.getSeconds()].join(" ");

console.log(show(new Date(2014, 6, 2, 12, 20, 60)));
console.log(show(new Date(2014, 11, 31, 23, 59, 59, 1000)));
console.log(show(new Date(2014, 0, 1, 25, 61, -1)));
console.log(show(new Date(2014, 14, 35)), show(new Date(2014, -1, 1)), show(new Date(2014, 0, 0)));
console.log(new Date(99, 0, 1).getFullYear(), new Date(100, 0, 1).getFullYear());
console.log(Date.UTC(70, 0, 1, 0, 0, 0, 1), Date.UTC(2014, 6, 2, 12, 20, 60), Date.UTC(2014, 6, 2, 12, 20.9));
console.log(new Date(2014, NaN, 1).getTime(), new Date(2014, 0, Infinity).getTime());
