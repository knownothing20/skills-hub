import { describe, it, expect } from 'vitest'
import { useImportPlan } from './useImportPlan'

describe('useImportPlan hook', () => {
  it('is exported as a function', () => {
    expect(typeof useImportPlan).toBe('function')
  })
})
