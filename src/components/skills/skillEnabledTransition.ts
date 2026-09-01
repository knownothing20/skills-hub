export type SkillEnabledInvoker = (
  command: string,
  args?: Record<string, unknown>,
) => Promise<unknown>

export function invokeManagedSkillEnabledTransition(
  invoke: SkillEnabledInvoker,
  skillId: string,
  enabled: boolean,
) {
  if (enabled) {
    return invoke('enable_skill_and_restore_targets', { skillId })
  }
  return invoke('set_skill_enabled', { skillId, enabled: false })
}
