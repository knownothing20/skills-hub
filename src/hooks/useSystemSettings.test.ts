import { describe, it, expect } from 'vitest'
import { useSystemSettings } from './useSystemSettings'

describe('useSystemSettings hook', () => {
  it('is exported as a function', () => {
    expect(typeof useSystemSettings).toBe('function')
  })
})
