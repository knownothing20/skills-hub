import { describe, expect, it } from 'vitest'
import {
  filterUpdateableSkills,
  isSkillUpdateable,
} from './skillUpdateability'

describe('skill updateability', () => {
  it('treats the backend field as authoritative for a single skill', () => {
    expect(isSkillUpdateable({ updateable: true })).toBe(true)
    expect(isSkillUpdateable({ updateable: false })).toBe(false)
  })

  it('filters bulk updates by the backend field instead of source path heuristics', () => {
    const skills = [
      {
        id: 'git-without-external-source',
        source_type: 'git',
        source_ref: 'https://example.com/repo.git',
        central_path: '/Users/example/.agents/skills/git-without-external-source',
        updateable: false,
      },
      {
        id: 'backend-approved-local-source',
        source_type: 'local',
        source_ref: '/Users/example/.agents/skills/backend-approved-local-source',
        central_path: '/Users/example/.agents/skills/backend-approved-local-source',
        updateable: true,
      },
      {
        id: 'ordinary-managed-skill',
        source_type: 'local',
        source_ref: '/Users/example/.codex/skills/ordinary-managed-skill',
        central_path: '/Users/example/.agents/skills/ordinary-managed-skill',
        updateable: false,
      },
    ]

    expect(filterUpdateableSkills(skills).map(({ id }) => id)).toEqual([
      'backend-approved-local-source',
    ])
  })
})
