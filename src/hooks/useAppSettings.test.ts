import { describe, it, expect } from 'vitest'
import {
  LANGUAGE_STORAGE_KEY,
  THEME_STORAGE_KEY,
  SKILL_VIEW_MODE_STORAGE_KEY,
  SIDEBAR_COLLAPSED_STORAGE_KEY,
} from './useAppSettings'

describe('useAppSettings keys and types', () => {
  it('exports expected storage keys', () => {
    expect(LANGUAGE_STORAGE_KEY).toBe('skills-language')
    expect(THEME_STORAGE_KEY).toBe('skills-theme')
    expect(SKILL_VIEW_MODE_STORAGE_KEY).toBe('skills-view-mode')
    expect(SIDEBAR_COLLAPSED_STORAGE_KEY).toBe('skills-sidebar-collapsed')
  })
})
