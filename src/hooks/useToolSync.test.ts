import { describe, it, expect } from 'vitest'
import { useToolSync } from './useToolSync'

describe('useToolSync hook', () => {
  it('is exported as a function', () => {
    expect(typeof useToolSync).toBe('function')
  })
})
