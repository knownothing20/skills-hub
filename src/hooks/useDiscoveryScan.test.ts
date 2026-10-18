import { describe, it, expect } from 'vitest'
import { useDiscoveryScan } from './useDiscoveryScan'

describe('useDiscoveryScan hook', () => {
  it('is exported as a function', () => {
    expect(typeof useDiscoveryScan).toBe('function')
  })
})
