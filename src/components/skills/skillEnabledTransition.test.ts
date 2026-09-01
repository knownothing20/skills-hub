import { describe, expect, it, vi } from 'vitest'
import { invokeManagedSkillEnabledTransition } from './skillEnabledTransition'

describe('invokeManagedSkillEnabledTransition', () => {
  it('uses the atomic backend command when enabling', async () => {
    const invoke = vi.fn(async () => undefined)

    await invokeManagedSkillEnabledTransition(invoke, 'skill-a', true)

    expect(invoke).toHaveBeenCalledWith('enable_skill_and_restore_targets', {
      skillId: 'skill-a',
    })
    expect(invoke).toHaveBeenCalledTimes(1)
  })

  it('keeps the existing command for disabling', async () => {
    const invoke = vi.fn(async () => undefined)

    await invokeManagedSkillEnabledTransition(invoke, 'skill-a', false)

    expect(invoke).toHaveBeenCalledWith('set_skill_enabled', {
      skillId: 'skill-a',
      enabled: false,
    })
  })

  it('routes every item in a bulk enable through one atomic call', async () => {
    const invoke = vi.fn(
      async (command: string, args?: Record<string, unknown>) => {
        void command
        void args
      },
    )

    for (const skillId of ['skill-a', 'skill-b', 'skill-c']) {
      await invokeManagedSkillEnabledTransition(invoke, skillId, true)
    }

    expect(invoke).toHaveBeenCalledTimes(3)
    expect(invoke.mock.calls.map(([command, args]) => [command, args])).toEqual([
      ['enable_skill_and_restore_targets', { skillId: 'skill-a' }],
      ['enable_skill_and_restore_targets', { skillId: 'skill-b' }],
      ['enable_skill_and_restore_targets', { skillId: 'skill-c' }],
    ])
    expect(invoke.mock.calls.some(([command]) => command === 'sync_skill_to_tool')).toBe(false)
  })
})
