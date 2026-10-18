import { describe, it, expect } from 'vitest'
import { useSkillFilters } from './useSkillFilters'

describe('useSkillFilters hook', () => {
  it('is exported as a function', () => {
    expect(typeof useSkillFilters).toBe('function')
  })
})
