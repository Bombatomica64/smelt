// A default on an arrow const's parameter behaves like one on a function
// declaration: an omitted argument takes the default (evaluated per call, in
// the callee), not the type's zero value.

export const cluster = (list: number[], size: number = 2): number[][] => {
  const count = Math.ceil(list.length / size)
  const out: number[][] = []
  for (let i = 0; i < count; i++) {
    out.push(list.slice(i * size, i * size + size))
  }
  return out
}

const greet = (name: string, greeting: string = 'hello'): string => `${greeting}, ${name}`

const scaled = (value: number, factor: number = value * 2): number => value * factor

console.log(cluster([1, 2, 3, 4, 5]).length)
console.log(cluster([1, 2, 3, 4, 5], 5).length)
console.log(greet('ada'))
console.log(greet('ada', 'hi'))
console.log(scaled(3))
console.log(scaled(3, 1))
