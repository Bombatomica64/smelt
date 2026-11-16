import { describe, test, expect } from 'vitest'

// Deep equality of records that are not compared through their views: each
// compares field with field at its own type (`SmeltDeepEq`), and nothing is
// erased to be compared. Every case is checked against vitest.

// A generic record written through a second reference: one shared object.
interface Box<T> { value: T; label: string }
function bump(box: Box<number>): Box<number> { box.value += 1; return box }
function boxes(first: number): Box<number>[] {
  const list: Box<number>[] = [{ value: 0, label: 'a' }, { value: 2, label: 'b' }]
  const head = list[0]
  head.value = first
  return list
}
function textBoxes(first: string): Box<string>[] {
  const list: Box<string>[] = [{ value: '', label: 'a' }]
  const head = list[0]
  head.value = first
  return list
}
function listBoxes(last: number): Box<number[]>[] {
  const list: Box<number[]>[] = [{ value: [], label: 'a' }]
  const head = list[0]
  head.value = [1, NaN, last]
  return list
}
function optionalBox(value: number, present: boolean): Box<number> | undefined {
  const box: Box<number> = { value: 0, label: 'a' }
  const same = box
  same.value = value
  return present ? box : undefined
}
function boxPair(value: number): [Box<number>, string] {
  const box: Box<number> = { value: 0, label: 'a' }
  const same = box
  same.value = value
  return [box, 'pair']
}
function boxesByLabel(value: number): Record<string, Box<number>> {
  const out: Record<string, Box<number>> = {}
  for (const box of boxes(value)) { out[box.label] = box }
  return out
}

// A generic record whose type argument is another record.
interface Person { name: string; age: number }
function birthday(person: Person): Person { person.age += 1; return person }
function personBoxes(age: number): Box<Person>[] {
  const person: Person = { name: 'a', age }
  birthday(person)
  const list: Box<Person>[] = [{ value: person, label: 'p' }]
  const head = list[0]
  head.label = 'q'
  return list
}
function nestedBoxes(value: number): Box<Box<number>>[] {
  const inner: Box<number> = { value: 0, label: 'inner' }
  const same = inner
  same.value = value
  const list: Box<Box<number>>[] = [{ value: inner, label: 'outer' }]
  return list
}

// A by-value struct that holds a shared record.
interface Team { lead: Person; size: number }
function teams(age: number, size: number): Team[] {
  const lead: Person = { name: 'a', age }
  birthday(lead)
  const team: Team = { lead, size }
  return [team]
}
// A by-value struct that holds a list of generic records and a number.
interface Shelf { boxes: Box<number>[]; weight: number }
function shelves(first: number, weight: number): Shelf[] {
  const shelf: Shelf = { boxes: boxes(first), weight }
  return [shelf]
}

// An accessor-backed record: the getter's value is the property.
interface Measured { readonly size: number; unit: string }
function measured(size: number, unit: string): Measured[] {
  const one: Measured = { get size() { return size }, unit }
  return [one]
}
// An accessor-backed record holding a shared record.
interface Owned { readonly owner: Person; tag: string }
function owned(age: number): Owned[] {
  const person: Person = { name: 'a', age }
  birthday(person)
  const one: Owned = { get owner() { return person }, tag: 't' }
  return [one]
}

// A record seen as another record type: the handle stands for the object it
// was built as, and deep equality compares that object's properties.
interface Named { name: string }
function asNamed(named: Named): Named { return named }
function namedPeople(age: number): Named[] {
  const person: Person = { name: 'a', age }
  return [asNamed(person)]
}
function plainNamed(): Named[] {
  const named: Named = { name: 'a' }
  return [asNamed(named)]
}
interface Badge { holder: Named; level: number }
function badges(age: number): Badge[] {
  const person: Person = { name: 'a', age }
  const badge: Badge = { holder: asNamed(person), level: 1 }
  return [badge]
}

// Cycles through a generic record.
interface Ring<T> { value: T; next: Ring<T> | null }
function ring(value: number, length: number): Ring<number>[] {
  const first: Ring<number> = { value, next: null }
  let last = first
  for (let index = 1; index < length; index++) {
    const node: Ring<number> = { value, next: null }
    last.next = node
    last = node
  }
  last.next = first
  return [first]
}
function chain(value: number): Ring<number>[] {
  const first: Ring<number> = { value, next: null }
  const second: Ring<number> = { value, next: null }
  first.next = second
  return [first]
}

describe('typed deep equality', () => {
  test('T_generic_list', () => { expect(boxes(1)).toEqual(boxes(1)) })
  test('F_generic_list_other_value', () => { expect(boxes(1)).toEqual(boxes(3)) })
  test('F_generic_list_other_length', () => { expect(boxes(1)).toEqual(boxes(1).slice(1)) })
  test('T_generic_same_list_twice', () => { const list = boxes(1); expect(list).toEqual(list) })
  test('T_generic_nan', () => { expect(boxes(NaN)).toEqual(boxes(NaN)) })
  test('T_generic_string_argument', () => { expect(textBoxes('x')).toEqual(textBoxes('x')) })
  test('F_generic_string_argument', () => { expect(textBoxes('x')).toEqual(textBoxes('y')) })
  test('T_generic_list_argument', () => { expect(listBoxes(3)).toEqual(listBoxes(3)) })
  test('F_generic_list_argument', () => { expect(listBoxes(3)).toEqual(listBoxes(4)) })
  test('T_generic_optional', () => { expect(optionalBox(1, true)).toEqual(optionalBox(1, true)) })
  test('F_generic_optional_other_value', () => { expect(optionalBox(1, true)).toEqual(optionalBox(2, true)) })
  test('F_generic_optional_absent', () => { expect(optionalBox(1, true)).toEqual(optionalBox(1, false)) })
  test('T_generic_tuple', () => { expect(boxPair(1)).toEqual(boxPair(1)) })
  test('F_generic_tuple', () => { expect(boxPair(1)).toEqual(boxPair(2)) })
  test('T_generic_dict', () => { expect(boxesByLabel(1)).toEqual(boxesByLabel(1)) })
  test('F_generic_dict', () => { expect(boxesByLabel(1)).toEqual(boxesByLabel(2)) })
  test('T_generic_record_argument', () => { expect(personBoxes(1)).toEqual(personBoxes(1)) })
  test('F_generic_record_argument', () => { expect(personBoxes(1)).toEqual(personBoxes(2)) })
  test('T_generic_nested', () => { expect(nestedBoxes(1)).toEqual(nestedBoxes(1)) })
  test('F_generic_nested', () => { expect(nestedBoxes(1)).toEqual(nestedBoxes(2)) })
  test('T_generic_identity', () => { const list = boxes(1); expect(bump(list[1])).toBe(list[1]) })
  test('F_generic_identity', () => { expect(boxes(1)[1]).toBe(boxes(1)[1]) })
  test('T_struct_holding_record', () => { expect(teams(1, 3)).toEqual(teams(1, 3)) })
  test('F_struct_holding_record_other_record', () => { expect(teams(1, 3)).toEqual(teams(2, 3)) })
  test('F_struct_holding_record_other_field', () => { expect(teams(1, 3)).toEqual(teams(1, 4)) })
  test('T_struct_holding_generic_list', () => { expect(shelves(1, NaN)).toEqual(shelves(1, NaN)) })
  test('F_struct_holding_generic_list', () => { expect(shelves(1, 5)).toEqual(shelves(2, 5)) })
  test('T_accessor', () => { expect(measured(2, 'm')).toEqual(measured(2, 'm')) })
  test('F_accessor', () => { expect(measured(2, 'm')).toEqual(measured(3, 'm')) })
  test('T_accessor_nan', () => { expect(measured(NaN, 'm')).toEqual(measured(NaN, 'm')) })
  test('T_accessor_holding_record', () => { expect(owned(1)).toEqual(owned(1)) })
  test('F_accessor_holding_record', () => { expect(owned(1)).toEqual(owned(2)) })
  test('T_view_handle', () => { expect(namedPeople(1)).toEqual(namedPeople(1)) })
  test('F_view_handle_other_hidden_property', () => { expect(namedPeople(1)).toEqual(namedPeople(2)) })
  test('F_view_handle_against_its_own_type', () => { expect(namedPeople(1)).toEqual(plainNamed()) })
  test('T_view_handle_of_its_own_type', () => { expect(plainNamed()).toEqual(plainNamed()) })
  test('T_struct_holding_view_handle', () => { expect(badges(1)).toEqual(badges(1)) })
  test('F_struct_holding_view_handle', () => { expect(badges(1)).toEqual(badges(2)) })
  test('T_generic_cycle', () => { expect(ring(1, 1)).toEqual(ring(1, 1)) })
  test('T_generic_cycle_of_two', () => { expect(ring(1, 2)).toEqual(ring(1, 2)) })
  test('F_generic_cycle_other_value', () => { expect(ring(1, 1)).toEqual(ring(2, 1)) })
  test('F_generic_cycle_of_another_length', () => { expect(ring(1, 1)).toEqual(ring(1, 2)) })
  test('F_generic_cycle_against_chain', () => { expect(ring(1, 2)).toEqual(chain(1)) })
})

// A callable record compares as a function does: by identity.
interface Counter { (step: number): number; count: number }
function counter(count: number): Counter {
  const call = (step: number): number => step + count
  call.count = count
  return call
}
function counters(count: number): Counter[] { return [counter(count)] }

describe('callable deep equality', () => {
  test('T_callable_same_list_twice', () => { const all = counters(1); expect(all).toEqual(all) })
  test('T_callable_same_function', () => { const one = counter(1); expect([one]).toEqual([one]) })
  test('F_callable_other_function', () => { expect(counters(1)).toEqual(counters(1)) })
})
