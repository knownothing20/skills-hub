import type { ManagedSkill } from './types'

type UpdateableSkill = Pick<ManagedSkill, 'updateable'>

export const isSkillUpdateable = (skill: UpdateableSkill) =>
  skill.updateable === true

export const filterUpdateableSkills = <Skill extends UpdateableSkill>(
  skills: Skill[],
) => skills.filter(isSkillUpdateable)
