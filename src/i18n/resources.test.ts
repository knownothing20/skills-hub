import { describe, expect, it } from 'vitest'
import tauriConfig from '../../src-tauri/tauri.conf.json'
import { resources } from './resources'

describe('product name', () => {
  it('uses Skills Hub consistently in both locales', () => {
    expect(resources.en.translation.appName).toBe('Skills Hub')
    expect(resources.zh.translation.appName).toBe('Skills Hub')
  })

  it('keeps the visible bundle name while preserving the data identifier', () => {
    expect(tauriConfig.productName).toBe('Skills Hub')
    expect(tauriConfig.app.windows[0]?.title).toBe('Skills Hub')
    expect(tauriConfig.identifier).toBe('io.github.mcncarl.skillshub')
  })
})
