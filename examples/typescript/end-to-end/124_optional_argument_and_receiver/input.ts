// Omitted optional arguments, and `?.` builtin calls on an absent-able local.
//
// * A const-bound arrow with an optional parameter (`(a: string, b?: string)`)
//   called with fewer arguments took the "argument count mismatch" branch,
//   which dropped EVERY argument: `f('p')` ran as `f()`. An optional
//   parameter without a default is padded with `undefined`, like a defaulted
//   one is padded with its default (Hono's `getPattern(':id')`).
// * `s?.at(-1)` / `s?.indexOf(..)` on a `string | undefined` local never
//   reached the string methods (they are keyed on a `string` receiver): `at`
//   became a field read calling a default callback, `indexOf` was rejected.
//   The call is lowered on the narrowed local and wrapped as
//   `s present ? call : undefined` (Hono's `mergePath`).
//
// Every line below is diffed against Node.

const join = (a: string, b?: string): string => a + (b ?? '-')
console.log(join('p'), join('p', 'q'))

const mergePath = (base: string | undefined, sub: string | undefined): string =>
  `${base?.[0] === '/' ? '' : '/'}${base}${
    sub === '/' ? '' : `${base?.at(-1) === '/' ? '' : '/'}${sub?.[0] === '/' ? sub.slice(1) : sub}`
  }`
console.log(mergePath('/book/', '/hey'), mergePath('/', 'book'), mergePath('/', '/book'))

const words: (string | undefined)[] = ['/xy', undefined]
for (const w of words) {
  console.log(w?.at(-1), w?.slice(1), w?.toUpperCase(), w?.indexOf('x'), w?.startsWith('/'))
}
