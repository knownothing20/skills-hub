import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { isTauri, invokeTauri } from './useTauriInvoke'
import type { ManagedSkill } from '../components/skills/types'

export type SkillScopeState = Record<
  string,
  {
    scope: 'global' | 'project'
    projects: string[]
  }
>

export interface UseSkillScopeProps {
  managedSkills: ManagedSkill[]
  installedToolIds: string[]
  installedProjectToolIds: string[]
  loading: boolean
  setLoading: (loading: boolean) => void
  setLoadingStartAt: (time: number | null) => void
  setError: (error: string | null) => void
  loadManagedSkills: () => Promise<void>
}

const SKILL_SCOPE_STORAGE_KEY = 'skills-project-scope-state-v1'

export function useSkillScope({
  managedSkills,
  installedToolIds,
  installedProjectToolIds,
  loading,
  setLoading,
  setLoadingStartAt,
  setError,
  loadManagedSkills,
}: UseSkillScopeProps) {
  const { t } = useTranslation()

  const [skillScopeState, setSkillScopeState] = useState<SkillScopeState>({})
  const [scopeModalSkill, setScopeModalSkill] = useState<ManagedSkill | null>(null)
  const [recentProjects, setRecentProjects] = useState<string[]>([])

  useEffect(() => {
    if (typeof window === 'undefined') return
    try {
      const raw = window.localStorage.getItem(SKILL_SCOPE_STORAGE_KEY)
      if (raw) {
        setSkillScopeState(JSON.parse(raw) as SkillScopeState)
      }
    } catch {
      setSkillScopeState({})
    }
  }, [])

  useEffect(() => {
    if (typeof window === 'undefined') return
    try {
      window.localStorage.setItem(
        SKILL_SCOPE_STORAGE_KEY,
        JSON.stringify(skillScopeState),
      )
    } catch {
      // ignore storage failures
    }
  }, [skillScopeState])

  useEffect(() => {
    if (!isTauri) return
    invokeTauri<string[]>('get_recent_projects')
      .then((projects) => setRecentProjects(projects))
      .catch(() => {})
  }, [])

  const getSkillProjects = useCallback(
    (skill: ManagedSkill) => {
      const projects = new Set<string>()
      for (const target of skill.targets) {
        if ((target.scope ?? 'global') === 'project' && target.project_path) {
          projects.add(target.project_path)
        }
      }
      return Array.from(projects)
    },
    [],
  )

  const getSkillScope = useCallback(
    (skill: ManagedSkill): 'global' | 'project' => {
      const hasGlobalTarget = skill.targets.some(
        (target) => (target.scope ?? 'global') === 'global',
      )
      const hasProjectTarget = skill.targets.some(
        (target) => (target.scope ?? 'global') === 'project',
      )
      if (hasGlobalTarget && !hasProjectTarget) return 'global'
      if (hasProjectTarget && !hasGlobalTarget) return 'project'
      const stored = skillScopeState[skill.id]?.scope
      if (stored === 'global' || stored === 'project') return stored
      return hasProjectTarget ? 'project' : 'global'
    },
    [skillScopeState],
  )

  const setSkillScopeAndProjects = useCallback(
    (skillId: string, scope: 'global' | 'project', projects: string[]) => {
      const uniqueProjects = Array.from(new Set(projects.filter(Boolean)))
      setSkillScopeState((prev) => ({
        ...prev,
        [skillId]: {
          scope,
          projects: uniqueProjects,
        },
      }))
    },
    [],
  )

  const handleOpenScope = useCallback((skill: ManagedSkill) => {
    setScopeModalSkill(skill)
  }, [])

  const handleCloseScope = useCallback(() => {
    if (!loading) setScopeModalSkill(null)
  }, [loading])

  const handlePickProject = useCallback(async () => {
    try {
      if (!isTauri) throw new Error(t('errors.notTauri'))
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({
        directory: true,
        multiple: false,
        title: t('projectSync.selectProjectTitle'),
      })
      if (!selected || Array.isArray(selected)) return undefined
      return selected
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
      return undefined
    }
  }, [setError, t])

  const handleScopeChange = useCallback(
    async (nextScope: 'global' | 'project', nextProjects: string[]) => {
      const skill = scopeModalSkill
      if (!skill || loading) return
      const projects = Array.from(new Set(nextProjects.filter(Boolean)))
      const hasStaleTargets = skill.targets.some(
        (target) =>
          (target.scope ?? 'global') !== nextScope ||
          (nextScope === 'project' &&
            (target.scope ?? 'global') === 'project' &&
            (!target.project_path || !projects.includes(target.project_path))),
      )
      const activeTargets = skill.targets.filter(
        (target) =>
          (target.scope ?? 'global') !== nextScope ||
          (nextScope === 'project' &&
            (target.scope ?? 'global') === 'project' &&
            (!target.project_path || !projects.includes(target.project_path))),
      )
      const existingProjects = getSkillProjects(skill)
      const projectsChanged =
        projects.length !== existingProjects.length ||
        projects.some((project) => !existingProjects.includes(project))
      if (getSkillScope(skill) === nextScope && !hasStaleTargets && !projectsChanged) {
        return
      }

      setLoading(true)
      setLoadingStartAt(Date.now())
      setError(null)
      try {
        const seen = new Set<string>()
        for (const target of activeTargets) {
          const targetScope = target.scope ?? 'global'
          const key = `${target.tool}|${targetScope}|${target.project_path ?? ''}`
          if (seen.has(key)) continue
          seen.add(key)
          await invokeTauri('unsync_skill_from_tool', {
            skillId: skill.id,
            tool: target.tool,
            scope: targetScope,
            projectPath: target.project_path ?? undefined,
          })
        }
        if (nextScope === 'project' && projects.length > 0) {
          for (const toolId of installedProjectToolIds) {
            for (const projectPath of projects) {
              await invokeTauri('sync_skill_to_tool', {
                sourcePath: skill.central_path,
                skillId: skill.id,
                tool: toolId,
                name: skill.name,
                overwriteIfSameContent: true,
                scope: 'project',
                projectPath,
              })
            }
          }
        } else if (nextScope === 'global') {
          for (const toolId of installedToolIds) {
            await invokeTauri('sync_skill_to_tool', {
              sourcePath: skill.central_path,
              skillId: skill.id,
              tool: toolId,
              name: skill.name,
              overwriteIfSameContent: true,
              scope: 'global',
            })
          }
        }
        await loadManagedSkills()
        if (nextScope === 'project') {
          for (const projectPath of projects) {
            const saved = await invokeTauri<string[]>('save_recent_project', {
              projectPath,
            })
            setRecentProjects(saved)
          }
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
        await loadManagedSkills()
        return
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
      }

      setSkillScopeAndProjects(
        skill.id,
        nextScope,
        nextScope === 'project' ? projects : [],
      )
      setScopeModalSkill(null)
    },
    [
      getSkillProjects,
      getSkillScope,
      installedProjectToolIds,
      installedToolIds,
      loadManagedSkills,
      loading,
      scopeModalSkill,
      setError,
      setLoading,
      setLoadingStartAt,
      setSkillScopeAndProjects,
    ],
  )

  const currentScopeModalSkill = useMemo(() => {
    if (!scopeModalSkill) return null
    return (
      managedSkills.find((skill) => skill.id === scopeModalSkill.id) ??
      scopeModalSkill
    )
  }, [managedSkills, scopeModalSkill])

  return {
    skillScopeState,
    setSkillScopeState,
    scopeModalSkill,
    setScopeModalSkill,
    recentProjects,
    setRecentProjects,
    getSkillProjects,
    getSkillScope,
    setSkillScopeAndProjects,
    handleOpenScope,
    handleCloseScope,
    handlePickProject,
    handleScopeChange,
    currentScopeModalSkill,
  }
}
