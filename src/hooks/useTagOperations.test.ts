import { describe, it, expect } from 'vitest'
import { useTagOperations } from './useTagOperations'

describe('useTagOperations hook', () => {
  it('is exported as a function', () => {
    expect(typeof useTagOperations).toBe('function')
  })
})
