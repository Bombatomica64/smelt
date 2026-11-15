// `new (date.constructor)(value)` on a date builds a date from the value, as
// `new Date(value)` does: a time value, another date, an ISO string. This is
// how date-fns' `constructFrom` builds every date it returns.

interface DateConstructor<D extends Date> {
  new (value: number | string | Date): D;
}

function rebuild<D extends Date>(date: D, value: number | string | Date): D {
  return new (date.constructor as DateConstructor<D>)(value);
}

const base = new Date(0);
console.log(rebuild(base, 5).getTime());
console.log(rebuild(base, 86400000).toISOString());
console.log(rebuild(base, new Date(1000)).getTime());
console.log(rebuild(base, "2020-02-03T04:05:06.007Z").getTime());
console.log(rebuild(base, 1e20).getTime());
console.log(rebuild(base, 2.9).getTime());
