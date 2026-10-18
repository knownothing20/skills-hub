import { describe, it, expect } from 'vitest'
import { getGithubInfo, getSkillSourceLabel } from './useTauriInvoke'

describe('getGithubInfo', () => {
  it('parses standard GitHub HTTPS URL', () => {
    const result = getGithubInfo('https://github.com/owner/repo')
    expect(result).toEqual({ label: 'owner/repo', href: 'https://github.com/owner/repo' })
  })

  it('parses GitHub URL with .git suffix', () => {
    const result = getGithubInfo('https://github.com/owner/repo.git')
    expect(result).toEqual({ label: 'owner/repo', href: 'https://github.com/owner/repo' })
  })

  it('handles git+ prefix', () => {
    const result = getGithubInfo('git+https://github.com/owner/repo.git')
    expect(result).toEqual({ label: 'owner/repo', href: 'https://github.com/owner/repo' })
  })

  it('returns null for non-GitHub URL', () => {
    expect(getGithubInfo('https://gitlab.com/owner/repo')).toBeNull()
  })

  it('returns null for null/undefined input', () => {
    expect(getGithubInfo(null)).toBeNull()
    expect(getGithubInfo(undefined)).toBeNull()
  })

  it('returns null for incomplete URL', () => {
    expect(getGithubInfo('https://github.com/owner')).toBeNull()
  })

  it('handles URL with extra path segments', () => {
    const result = getGithubInfo('https://github.com/owner/repo/tree/main/subdir')
    expect(result).toEqual({ label: 'owner/repo', href: 'https://github.com/owner/repo' })
  })

  it('handles non-URL GitHub string via regex fallback', () => {
    const result = getGithubInfo('github.com/owner/repo')
    expect(result).toEqual({ label: 'owner/repo', href: 'https://github.com/owner/repo' })
  })
})

describe('getSkillSourceLabel', () => {
  it('returns source_ref for git skills', () => {
    const skill = { source_type: 'Git', source_ref: 'https://github.com/o/r', central_path: '/c/p' }
    expect(getSkillSourceLabel(skill)).toBe('https://github.com/o/r')
  })

  it('returns central_path for local skills', () => {
    const skill = { source_type: 'Local', source_ref: null, central_path: '/central/my-skill' }
    expect(getSkillSourceLabel(skill)).toBe('/central/my-skill')
  })

  it('returns central_path when git skill has no ref', () => {
    const skill = { source_type: 'Git', source_ref: null, central_path: '/central/my-skill' }
    expect(getSkillSourceLabel(skill)).toBe('/central/my-skill')
  })
})
