export const iterate = <T>(count: number, func: (v: T, i: number) => T, init: T) => {
  let value = init
  for (let i = 1; i <= count; i++) { value = func(value, i) }
  return value
}
export function pick<T>(a: T, b: T): T { return b }
