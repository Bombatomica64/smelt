// Annotation types TypeScript 7 evaluates that Smelt's own type lowering
// erased to `unknown`: with the embedded checker each binding keeps the type
// TypeScript proved (`string[]`, `number`), so the reads below are typed.

// A conditional type instantiated with a concrete argument.
type Words<T> = T extends string ? string[] : number
const words: Words<'a'> = ['x', 'y']

// Indexed-access types.
interface Config {
  port: number
  host: string
  tags: string[]
}
const port: Config['port'] = 8080
const tags: Config['tags'] = ['a', 'b', 'c']

// `ReturnType` of a method read through an indexed access.
class Store {
  get(key: string): number {
    return key.length
  }
}
const got: ReturnType<Store['get']> = new Store().get('abcd')

console.log(words.length, words.join('+'))
console.log(port + 1, tags.join(','), tags.length)
console.log(got * 2)
