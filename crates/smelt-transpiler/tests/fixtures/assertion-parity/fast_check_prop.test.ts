import { fc, test } from '@fast-check/vitest'
import { describe, expect } from 'vitest'
describe('prop', () => {
  test.prop([fc.integer()])('F_prop', (n) => { expect(n).toBe(n + 1) })
  test.prop([fc.integer()])('T_prop', (n) => { expect(n).toBe(n) })
})
