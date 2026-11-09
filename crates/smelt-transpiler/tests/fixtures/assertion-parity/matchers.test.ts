import { describe, test, expect, vi } from 'vitest'
describe('m', () => {
  test('F_nan', () => { expect(1).toBeNaN() })
  test('T_nan', () => { expect(NaN).toBeNaN() })
  test('F_close', () => { expect(1).toBeCloseTo(2) })
  test('T_close', () => { expect(0.1 + 0.2).toBeCloseTo(0.3) })
  test('F_snap', () => { expect(1).toMatchInlineSnapshot(`2`) })
  test('T_snap', () => { expect({ a: [1] }).toMatchInlineSnapshot(`
    {
      "a": [
        1,
      ],
    }
  `) })
  test('F_once', () => { const f = vi.fn(); f(1); expect(f).toHaveBeenCalledExactlyOnceWith(2) })
  test('T_once', () => { const f = vi.fn(); f(1); expect(f).toHaveBeenCalledExactlyOnceWith(1) })
  test('F_expect_fail', () => { expect.fail('x') })
  test('F_unknown', () => { (expect(1) as any).toBeWhatever(2) })
})
