import { renderToStaticMarkup } from 'react-dom/server'
import type { TFunction } from 'i18next'
import { describe, expect, it, vi } from 'vitest'
import UpdatesPage from './UpdatesPage'
import type { AutoUpdateConfigDto } from './types'

const translate = ((key: string) => key) as TFunction

const config: AutoUpdateConfigDto = {
  enabled: true,
  interval_hours: 24,
  schedule_type: 'interval',
  interval_value: 24,
  interval_unit: 'hours',
  daily_time: '03:00',
  local_skill_count: 0,
  protected_local_skill_count: 0,
  task_registered: true,
  task_status_detail: 'ready',
  last_run_at: 1,
  last_started_at: 1,
  last_finished_at: 2,
  last_status: 'ok',
  last_error: null,
  last_checked: 3,
  last_updated: 1,
  last_failed: 0,
  progress: {
    total: 3,
    succeeded: [{ skill_id: 'pua', name: 'pua' }],
    skipped: [
      {
        skill_id: 'managed-a',
        name: 'managed-a',
        reason: 'no external update source',
      },
      {
        skill_id: 'managed-b',
        name: 'managed-b',
        reason: 'no external update source',
      },
    ],
    failed: [],
    running: null,
    pending: [],
  },
}

describe('UpdatesPage skipped results', () => {
  it('shows skipped as a neutral result category with its reason', () => {
    const markup = renderToStaticMarkup(
      <UpdatesPage
        autoUpdateConfig={config}
        onAutoUpdateConfigChange={vi.fn()}
        onRunAutoUpdateNow={vi.fn()}
        autoUpdateTriggering={false}
        t={translate}
      />,
    )

    expect(markup).toMatch(
      /<span>autoUpdateSkippedShort<\/span><strong>2<\/strong>/,
    )
    expect(markup).toContain('autoUpdateResultEquation')
    expect(markup).toContain('autoUpdateSkippedTitle')
    expect(markup).toContain('managed-a')
    expect(markup).toContain('no external update source')
    expect(markup).not.toContain('updates-issue-block')
  })
})
