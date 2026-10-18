import { describe, it, expect } from 'vitest'
import { useInstallScopeTargets } from './useInstallScopeTargets'

describe('useInstallScopeTargets hook', () => {
  it('is exported as a function', () => {
    expect(typeof useInstallScopeTargets).toBe('function')
  })
})
