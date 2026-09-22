// A concise arrow body that CALLS a function item passes every argument the
// source wrote. A variadic host function such as `console.log` declares no
// fixed parameters, so wrapping it in a closure of its own arity dropped them.

const apply = (cb: (v: number) => void) => cb(5)
apply((v) => console.log('value', v))

const each = (items: string[], cb: (item: string, index: number) => void): void => {
  for (let i = 0; i < items.length; i++) cb(items[i], i)
}
each(['a', 'b'], (item, index) => console.log(index, item))

