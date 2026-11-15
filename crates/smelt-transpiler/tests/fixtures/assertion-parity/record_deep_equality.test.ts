import { describe, test, expect } from 'vitest'

// A record that is written through a second reference is one shared object in
// the generated Rust, and its handle's own equality is identity. Deep equality
// of a list, dictionary, tuple or optional of such records still compares
// their properties; `toBe` still compares the objects.
interface Person { name: string; age: number }
function birthday(person: Person): Person { person.age += 1; return person }
function people(): Person[] {
  const list: Person[] = [{ name: 'a', age: 1 }, { name: 'b', age: 5 }]
  birthday(list[0])
  return list
}
function byName(list: Person[]): Record<string, Person> {
  const out: Record<string, Person> = {}
  for (const person of list) { out[person.name] = person }
  return out
}
function first(list: Person[]): Person | undefined { return list[0] }
function pair(list: Person[]): [Person, number] { return [list[1], list.length] }

describe('record deep equality', () => {
  test('T_rec_list', () => { expect(people()).toEqual([{ name: 'a', age: 2 }, { name: 'b', age: 5 }]) })
  test('F_rec_list', () => { expect(people()).toEqual([{ name: 'a', age: 1 }, { name: 'b', age: 5 }]) })
  test('F_rec_list_length', () => { expect(people()).toEqual([{ name: 'a', age: 2 }]) })
  test('T_rec_dict', () => { expect(byName(people())).toEqual({ a: { name: 'a', age: 2 }, b: { name: 'b', age: 5 } }) })
  test('F_rec_dict', () => { expect(byName(people())).toEqual({ a: { name: 'a', age: 2 }, b: { name: 'c', age: 5 } }) })
  test('T_rec_optional', () => { expect(first(people())).toEqual({ name: 'a', age: 2 }) })
  test('F_rec_optional', () => { expect(first(people())).toEqual({ name: 'a', age: 9 }) })
  test('T_rec_tuple', () => { expect(pair(people())).toEqual([{ name: 'b', age: 5 }, 2]) })
  test('F_rec_tuple', () => { expect(pair(people())).toEqual([{ name: 'b', age: 6 }, 2]) })
  test('T_rec_same_list_twice', () => { const list = people(); expect(list).toEqual(list) })
  test('T_rec_identity', () => { const list = people(); expect(birthday(list[1])).toBe(list[1]) })
  test('F_rec_identity', () => { const list = people(); const other: Person = { name: 'b', age: 5 }; expect(list[1]).toBe(other) })
  test('F_rec_added_property', () => {
    const list = people()
    const open: any = list[1]
    open.extra = 1
    expect(list).toEqual([{ name: 'a', age: 2 }, { name: 'b', age: 5 }])
  })
})
