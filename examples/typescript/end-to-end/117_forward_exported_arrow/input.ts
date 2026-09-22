// An exported arrow const is the same lexical binding as a private one, so a
// body that calls an exported arrow declared FURTHER DOWN must reach the real
// function, not a placeholder that returns the type's default.

export const outer = (n: number): number => inner(n) + 1
export const inner = (n: number): number => n * 10

export const twice = async (n: number): Promise<number> => {
  const r = await later(n)
  return r * 2
}
export const later = async (n: number): Promise<number> => n + 1

export function viaFunction(n: number): number {
  return lastly(n) - 1
}
export const lastly = (n: number): number => n * n

console.log(outer(2))
console.log(viaFunction(4))
console.log(await twice(1))
