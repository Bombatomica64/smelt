import { describe, expect, test } from 'vitest'
import * as chai from 'chai'
import { expect as chaiExpect } from 'chai'

chai.should()
expect.extend({
  toBeEven(received: number) {
    return { pass: received % 2 === 0, message: () => `${received} is not even` }
  },
  toBeAbove(received: number, floor: number) {
    return { pass: received > floor, message: () => `${received} <= ${floor}` }
  },
})

class Boom extends Error {}
const throwsBoom = () => {
  throw new Boom('kaboom now')
}

describe('planned', () => {
  test('T_assertions_exact', () => { expect.assertions(2); expect(1).toBe(1); expect(2).toBe(2) })
  test('F_assertions_short', () => { expect.assertions(2); expect(1).toBe(1) })
  test('F_assertions_over', () => { expect.assertions(1); expect(1).toBe(1); expect(2).toBe(2) })
  test('T_has_assertions', () => { expect.hasAssertions(); expect(1).toBe(1) })
  test('F_has_assertions_none', () => { expect.hasAssertions() })
  test('F_assertions_early_return', () => { expect.assertions(2); expect(1).toBe(1); if (Date.now() > 0) return; expect(2).toBe(2) })
  test('T_assertions_early_return', () => { expect.assertions(1); expect(1).toBe(1); if (Date.now() > 0) return; expect(2).toBe(2) })
  test('T_assertions_loop', () => { expect.assertions(3); for (const n of [1, 2, 3]) expect(n).toBe(n) })
})

describe('extended', () => {
  test('T_even', () => { (expect(2) as any).toBeEven() })
  test('F_even', () => { (expect(3) as any).toBeEven() })
  test('T_not_even', () => { (expect(3).not as any).toBeEven() })
  test('F_not_even', () => { (expect(2).not as any).toBeEven() })
  test('T_above', () => { (expect(3) as any).toBeAbove(2) })
  test('F_above', () => { (expect(1) as any).toBeAbove(2) })
})

describe('throw with class and message', () => {
  test('T_class_and_text', () => { chaiExpect(throwsBoom).to.throw(Boom, 'kaboom') })
  test('F_class_wrong', () => { chaiExpect(throwsBoom).to.throw(RangeError, 'kaboom') })
  test('F_text_wrong', () => { chaiExpect(throwsBoom).to.throw(Boom, 'nope') })
  test('T_class_and_regex', () => { chaiExpect(throwsBoom).to.throw(Boom, /kaboom/) })
  test('F_regex_wrong', () => { chaiExpect(throwsBoom).to.throw(Boom, /nope/) })
})

describe('should interface', () => {
  test('T_should_equal', () => { (1 as any).should.equal(1) })
  test('F_should_equal', () => { (1 as any).should.equal(2) })
  test('T_should_not', () => { ('a' as any).should.not.equal('b') })
  test('T_should_chain', () => { ([1, 2] as any).should.be.an('array').and.have.lengthOf(2) })
  test('F_should_chain', () => { ([1, 2] as any).should.have.lengthOf(3) })
  test('T_should_throw', () => { (throwsBoom as any).should.throw(Boom, 'kaboom') })
  test('F_should_throw', () => { (((() => 1) as any).should.throw()) })
  test('F_should_not_throw', () => { (throwsBoom as any).should.not.throw() })
  test('T_should_deep', () => { ({ a: 1 } as any).should.deep.equal({ a: 1 }) })
})

const enabled = true
const disabled = false
const onlyEnabled = enabled ? test : test.skip
const onlyDisabled = disabled ? test : test.skip
describe('conditional alias', () => {
  onlyEnabled('F_alias_runs', () => { expect(1).toBe(2) })
  onlyEnabled('T_alias_runs', () => { expect(1).toBe(1) })
})
