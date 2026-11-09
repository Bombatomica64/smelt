import { describe, test, expect, vi } from 'vitest'

class Point {
  x: number
  constructor(x: number) { this.x = x }
  move(): Point { return new Point(this.x) }
}

async function rejecting(): Promise<void> {
  throw new Error('boom')
}

function fail(): never {
  throw new RangeError('bad')
}

describe('mock calls', () => {
  test('T_times', () => { const f = vi.fn(); f(1); f(2); expect(f).toHaveBeenCalledTimes(2) })
  test('F_times', () => { const f = vi.fn(); f(1); expect(f).toHaveBeenCalledTimes(2) })
  test('T_with', () => { const f = vi.fn(); f(1, 'a'); f(2, 'b'); expect(f).toHaveBeenCalledWith(1, 'a') })
  test('F_with', () => { const f = vi.fn(); f(1, 'a'); expect(f).toHaveBeenCalledWith(1, 'b') })
  test('T_last', () => { const f = vi.fn(); f(1); f(2); expect(f).toHaveBeenLastCalledWith(2) })
  test('F_last', () => { const f = vi.fn(); f(1); f(2); expect(f).toHaveBeenLastCalledWith(1) })
  test('T_resolved', async () => { const f = vi.fn(async () => 5); await f(); expect(f).toHaveLastResolvedWith(5) })
  test('F_resolved', async () => { const f = vi.fn(async () => 5); await f(); expect(f).toHaveLastResolvedWith(6) })
  test('T_mock_calls', () => { const f = vi.fn(); f(); f(); expect(f.mock.calls.length).toBe(2) })
  test('F_mock_calls', () => { const f = vi.fn(); f(); expect(f.mock.calls.length).toBe(2) })
  test('T_chain', async () => {
    const f = vi.fn().mockRejectedValueOnce(new Error('failure')).mockResolvedValue('success')
    let first = ''
    try { await f() } catch (e) { first = (e as Error).message }
    expect(first).toBe('failure')
    expect(await f()).toBe('success')
    expect(f).toHaveBeenCalledTimes(2)
  })
  test('F_nonmock_with', () => { expect({}).toHaveBeenCalledWith(1) })
  test('F_nonmock_times', () => { expect({}).toHaveBeenCalledTimes(0) })
  test('T_catch_assert', () => { try { fail() } catch (e) { expect(e instanceof RangeError).toBe(true) } })
  test('F_catch_assert', () => { try { fail() } catch (e) { expect(e instanceof TypeError).toBe(true) } })
  test('T_equal_tuples', () => {
    const actual: Array<[number, string]> = [[1, 'a'], [2, 'b']]
    expect(actual).toEqual([[1, 'a'], [2, 'b']])
  })
  test('F_equal_tuples', () => {
    const actual: Array<[number, string]> = [[1, 'a'], [2, 'b']]
    expect(actual).toEqual([[1, 'a'], [2, 'c']])
  })
  test('T_equal_class', () => { const a = new Point(1); expect(a.move()).toEqual(a) })
  test('F_equal_class', () => { const a = new Point(1); expect(new Point(2)).toEqual(a) })
  test('F_identity_class', () => { const a = new Point(1); expect(a.move()).toBe(a) })
  test('T_rejects', async () => { await expect(rejecting()).rejects.toThrow('boom') })
  test('F_rejects_text', async () => { await expect(rejecting()).rejects.toThrow('other') })
  test('F_rejects_resolved', async () => { await expect(Promise.resolve(1)).rejects.toThrow('boom') })
})
