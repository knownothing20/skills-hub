import { describe, it, expect } from 'vitest'
import { useSkillImport } from './useSkillImport'

describe('useSkillImport hook', () => {
  it('is exported as a function', () => {
    expect(typeof useSkillImport).toBe('function')
  })
})
