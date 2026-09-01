import { describe, expect, it } from 'vitest'
import {
  resolveSkillIconSpec,
  semanticIcon,
  semanticSkillIconKeys,
} from './skillIconRegistry'

describe('resolveSkillIconSpec', () => {
  it.each([
    ['security-audit', 'shield', 'emerald'],
    ['invoice-pdf', 'file-pdf', 'red'],
    ['team-research', 'search', 'blue'],
    ['meeting-transcript', 'waveform', 'cyan'],
    ['video-caption-editor', 'film', 'violet'],
    ['product-image-design', 'image', 'magenta'],
    ['project-database', 'database', 'indigo'],
    ['release-publisher', 'git-branch', 'indigo'],
    ['workflow-automation', 'flow', 'blue'],
    ['learning-tutorial', 'graduation', 'blue'],
  ])('maps %s to the generic %s semantic icon', (name, key, tone) => {
    expect(resolveSkillIconSpec(name)).toMatchObject({
      kind: 'semantic',
      key,
      tone,
      origin: 'semantic',
    })
  })

  it('normalizes case and surrounding whitespace before matching', () => {
    expect(resolveSkillIconSpec('  TEAM-RESEARCH  ')).toMatchObject({
      key: 'search',
      origin: 'semantic',
    })
  })

  it('uses a neutral package fallback for unknown Skills', () => {
    expect(resolveSkillIconSpec('future-capability')).toEqual({
      kind: 'semantic',
      key: 'package',
      tone: 'slate',
      origin: 'fallback',
    })
  })

  it('keeps the semantic icon catalog unique', () => {
    expect(new Set(semanticSkillIconKeys).size).toBe(semanticSkillIconKeys.length)
  })

  it('validates semantic definitions at runtime as well as at compile time', () => {
    expect(semanticIcon('search', 'blue')).toEqual({
      kind: 'semantic',
      key: 'search',
      tone: 'blue',
    })
    expect(() => semanticIcon('unknown' as 'search')).toThrow('Unknown semantic Skill icon')
    expect(() => semanticIcon('search', 'unknown' as 'blue')).toThrow('Unknown Skill icon tone')
  })
})
