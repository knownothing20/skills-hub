import { describe, it, expect } from 'vitest'
import { useSkillLifecycle } from './useSkillLifecycle'

describe('useSkillLifecycle hook', () => {
  it('is exported as a function', () => {
    expect(typeof useSkillLifecycle).toBe('function')
  })
})
