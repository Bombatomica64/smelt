import { describe, test, expect } from 'vitest'
// `toEqual` skips own properties holding `undefined` on both sides;
// `toStrictEqual` counts them. The operands are erased (`unknown`), so the
// harness's walk, not a typed comparison, decides.
const pick = (value: unknown): unknown => value
describe('undefined properties', () => {
  test('T_equal_ignores_undefined', () => { expect(pick({ n: undefined, s: 'x' })).toEqual({ a: undefined, s: 'x' }) })
  test('T_equal_ignores_missing', () => { expect(pick({ s: 'x', n: undefined })).toEqual({ s: 'x' }) })
  test('T_equal_nested', () => { expect(pick({ inner: { v: 1, w: undefined } })).toEqual({ inner: { v: 1 } }) })
  test('F_equal_value_differs', () => { expect(pick({ n: undefined, s: 'x' })).toEqual({ s: 'y' }) })
  test('F_equal_extra_defined', () => { expect(pick({ s: 'x', t: 1 })).toEqual({ s: 'x' }) })
  test('F_strict_counts_undefined', () => { expect(pick({ n: undefined, s: 'x' })).toStrictEqual({ s: 'x' }) })
  test('T_strict_same', () => { expect(pick({ n: undefined, s: 'x' })).toStrictEqual({ n: undefined, s: 'x' }) })
  test('F_undefined_is_not_null', () => { expect(pick(undefined)).toEqual(null) })
})
