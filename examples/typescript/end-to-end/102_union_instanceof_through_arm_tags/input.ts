// `x instanceof C` on a value whose static type is a generated union is the
// discriminant test for the arms that ARE that class — the same rule the
// `typeof` probe on a union already follows.
//
// The classes below all lower to their own representation rather than to a
// nominal class: a `Promise<T>` is a future, an array is a list, a `Map` is a
// keyed map. Asking only "is this arm a class named `Promise`?" answered no for
// every one of them, and no matching arm is emitted as a constant `false` — so
// the guarded branch became dead code and the program silently took the other
// one. Nothing here fails to compile; only the output was wrong.

async function label(value: string | Promise<string>): Promise<string> {
  if (value instanceof Promise) {
    return `deferred:${await value}`
  }
  return `direct:${value}`
}

function size(value: string | string[]): string {
  if (value instanceof Array) {
    return `list:${value.length}`
  }
  return `text:${value.length}`
}

function lookup(value: Map<string, number> | number): string {
  if (value instanceof Map) {
    return `map:${value.size}`
  }
  return `num:${value}`
}

async function run(): Promise<void> {
  console.log(await label('now'))
  console.log(await label(Promise.resolve('later')))
  console.log(size('abc'))
  console.log(size(['a', 'b']))
  console.log(lookup(7))
  console.log(lookup(new Map([['a', 3]])))
}

run()
