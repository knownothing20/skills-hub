import { renderToStaticMarkup } from 'react-dom/server'
import type { TFunction } from 'i18next'
import { describe, expect, it, vi } from 'vitest'
import SkillCard from './SkillCard'
import type { ManagedSkill } from './types'

const translate = ((key: string) => key) as TFunction

const createSkill = (updateable: boolean): ManagedSkill => ({
  id: 'example',
  name: 'example',
  description: 'Example Skill',
  source_type: 'git',
  source_ref: 'https://example.com/repo.git',
  has_external_source: true,
  central_path: '/Users/example/.agents/skills/example',
  created_at: 1,
  updated_at: 1,
  enabled: true,
  updateable,
  status: 'ok',
  tags: [],
  targets: [],
})

const renderCard = (skill: ManagedSkill) => renderToStaticMarkup(
  <SkillCard
    skill={skill}
    installedTools={[]}
    loading={false}
    bulkMode={false}
    bulkSelected={false}
    getGithubInfo={() => null}
    getSkillSourceLabel={() => 'source'}
    formatRelative={() => 'now'}
    onUpdate={vi.fn()}
    onDelete={vi.fn()}
    onToggleEnabled={vi.fn()}
    onToggleTool={vi.fn()}
    onOpenScope={vi.fn()}
    onOpenDetail={vi.fn()}
    onEditTags={vi.fn()}
    onToggleBulkSelection={vi.fn()}
    getSkillScope={() => 'global'}
    getSkillProjects={() => []}
    t={translate}
  />,
)

describe('SkillCard update action', () => {
  it('disables update when the backend marks a Git-looking skill as not updateable', () => {
    const markup = renderCard(createSkill(false))

    expect(markup).toMatch(
      /<button type="button" disabled="" aria-label="updateSourceUnavailable"[^>]*>/,
    )
  })

  it('enables update when the backend marks the skill as updateable', () => {
    const markup = renderCard(createSkill(true))

    expect(markup).toContain('aria-label="update" title="update"')
    expect(markup).not.toMatch(
      /<button type="button" disabled="" aria-label="update"[^>]*>/,
    )
  })

  it('passes backend Skill icon metadata through to the icon renderer', () => {
    const skill = {
      ...createSkill(true),
      icon_data_url: 'data:image/png;base64,iVBORw0KGgo=',
      brand_color: '#123ABC',
    }
    const markup = renderCard(skill)

    expect(markup).toContain('data-icon-source="metadata"')
    expect(markup).toContain('--skill-icon-color:#123ABC')
    expect(markup.match(/<img/g)).toHaveLength(1)
  })
})
