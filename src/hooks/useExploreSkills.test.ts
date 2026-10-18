import { describe, it, expect } from 'vitest'
import { useExploreSkills } from './useExploreSkills'

describe('useExploreSkills hook', () => {
  it('is exported as a function', () => {
    expect(typeof useExploreSkills).toBe('function')
  })
})
