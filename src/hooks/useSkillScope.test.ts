import { describe, it, expect } from 'vitest'
import { useSkillScope } from './useSkillScope'

describe('useSkillScope hook', () => {
  it('is exported as a function', () => {
    expect(typeof useSkillScope).toBe('function')
  })
})
