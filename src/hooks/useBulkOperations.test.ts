import { describe, it, expect } from 'vitest'
import { useBulkOperations } from './useBulkOperations'

describe('useBulkOperations hook', () => {
  it('is exported as a function', () => {
    expect(typeof useBulkOperations).toBe('function')
  })
})
