import { describe, it, expect } from 'vitest'
import { useAutoUpdate } from './useAutoUpdate'

describe('useAutoUpdate hook', () => {
  it('is exported as a function', () => {
    expect(typeof useAutoUpdate).toBe('function')
  })
})
