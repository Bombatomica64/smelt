import { describe, test, expect } from 'vitest'
import fc from 'fast-check'
describe('fc', () => {
  test('F_fc_expect', () => { fc.assert(fc.property(fc.integer(), (n) => { expect(n).toBe(n + 1) })) })
  test('T_fc_expect', () => { fc.assert(fc.property(fc.integer(), (n) => { expect(n).toBe(n) })) })
  test('F_fc_ret', () => { fc.assert(fc.property(fc.integer(), (n) => n !== n)) })
  test('T_fc_ret', () => { fc.assert(fc.property(fc.integer({min:1,max:5}), (n) => n >= 1 && n <= 5)) })
  test('F_fc_arr', () => { fc.assert(fc.property(fc.array(fc.integer()), (a) => a.length < 3)) })
  test('T_fc_arr', () => { fc.assert(fc.property(fc.array(fc.integer(),{maxLength:2}), (a) => a.length < 3)) })
  test('T_fc_map', () => { fc.assert(fc.property(fc.integer({min:0,max:9}).map((n) => n * 2), (n) => n % 2 === 0)) })
  test('F_fc_unmodelled', () => { (fc as any).frob() })
})
