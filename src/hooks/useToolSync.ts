import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { invokeTauri } from './useTauriInvoke'
import {
  getToolSyncState,
  isActiveSkillTarget,
} from '../components/skills/skillSyncStatus'
import type { ManagedSkill, ToolOption } from '../components/skills/types'

export interface UseToolSyncProps {
  loading: boolean
  setLoading: (loading: boolean) => void
  setLoadingStartAt: (time: number | null) => void
  setError: (error: string | null) => void
  setActionMessage: (msg: string | null) => void
  setSuccessToastMessage: (msg: string | null) => void
  loadManagedSkills: () => Promise<void>
  tools: ToolOption[]
  getSkillScope: (skill: ManagedSkill) => 'global' | 'project'
  getSkillProjects: (skill: ManagedSkill) => string[]
  toolSupportsProjectScope: (toolId: string) => boolean
  setScopeModalSkill: (skill: ManagedSkill | null) => void
  sharedToolIdsByToolId: Record<string, string[]>
  toolLabelById: Record<string, string>
}

export interface PendingToolToggle {
  skill: ManagedSkill
  toolId: string
  action: 'sync' | 'unsync'
}

export interface PendingSharedToggle {
  skill: ManagedSkill
  toolId: string
  affectedToolIds?: string[]
}

export function useToolSync({
  loading,
  setLoading,
  setLoadingStartAt,
  setError,
  setActionMessage,
  setSuccessToastMessage,
  loadManagedSkills,
  tools,
  getSkillScope,
  getSkillProjects,
  toolSupportsProjectScope,
  setScopeModalSkill,
  sharedToolIdsByToolId,
  toolLabelById,
}: UseToolSyncProps) {
  const { t } = useTranslation()

  const [pendingSharedToggle, setPendingSharedToggle] =
    useState<PendingSharedToggle | null>(null)
  const [pendingToolToggle, setPendingToolToggle] =
    useState<PendingToolToggle | null>(null)

  const runToggleToolForSkill = useCallback(
    async (
      skill: ManagedSkill,
      toolId: string,
      options?: {
        targetAction?: 'sync' | 'unsync'
        syncMode?: 'copy' | 'junction'
      },
    ) => {
      if (loading) return
      if (skill.enabled === false) {
        toast.error(t('bulk.enableBeforeSync'))
        return
      }
      const toolLabel = tools.find((tool) => tool.id === toolId)?.label ?? toolId
      const skillScope = getSkillScope(skill)
      const projects = getSkillProjects(skill)
      if (skillScope === 'project') {
        if (!toolSupportsProjectScope(toolId)) {
          setError(t('projectSync.unsupportedTool', { tool: toolLabel }))
          return
        }
        if (projects.length === 0) {
          setError(t('projectSync.noProjectsForSync'))
          setScopeModalSkill(skill)
          return
        }
      }
      const matchingTargets = skill.targets.filter(
        (target) =>
          target.tool === toolId &&
          (target.scope ?? 'global') === skillScope &&
          isActiveSkillTarget(target),
      )
      const synced = getToolSyncState(skill, toolId, skillScope) === 'synced'
      const shouldUnsync = options?.targetAction
        ? options.targetAction === 'unsync'
        : synced
      const isSwitchingMode = synced && !shouldUnsync && options?.syncMode != null

      setLoading(true)
      setLoadingStartAt(Date.now())
      setError(null)
      try {
        if (shouldUnsync) {
          setActionMessage(
            t('actions.unsyncing', { name: skill.name, tool: toolLabel }),
          )
          if (skillScope === 'project') {
            const targetProjects = Array.from(
              new Set(
                matchingTargets
                  .map((target) => target.project_path)
                  .filter((path): path is string => Boolean(path)),
              ),
            )
            for (const projectPath of targetProjects) {
              await invokeTauri('unsync_skill_from_tool', {
                skillId: skill.id,
                tool: toolId,
                scope: 'project',
                projectPath,
              })
            }
          } else {
            await invokeTauri('unsync_skill_from_tool', {
              skillId: skill.id,
              tool: toolId,
              scope: 'global',
            })
          }
        } else {
          setActionMessage(
            isSwitchingMode
              ? `正在切换「${skill.name}」在 ${toolLabel} 的同步模式...`
              : t('actions.syncing', { name: skill.name, tool: toolLabel }),
          )
          if (skillScope === 'project') {
            for (const projectPath of projects) {
              await invokeTauri('sync_skill_to_tool', {
                sourcePath: skill.central_path,
                skillId: skill.id,
                tool: toolId,
                name: skill.name,
                overwriteIfSameContent: true,
                overwrite: Boolean(isSwitchingMode),
                scope: 'project',
                projectPath,
                syncMode: options?.syncMode,
              })
            }
          } else {
            await invokeTauri('sync_skill_to_tool', {
              sourcePath: skill.central_path,
              skillId: skill.id,
              tool: toolId,
              name: skill.name,
              overwriteIfSameContent: true,
              overwrite: Boolean(isSwitchingMode),
              scope: 'global',
              syncMode: options?.syncMode,
            })
          }
        }
        const statusText = shouldUnsync
          ? t('status.syncDisabled')
          : isSwitchingMode
            ? `已切换为${options?.syncMode === 'copy' ? '实体副本' : '软链接'}`
            : t('status.syncEnabled')
        setActionMessage(statusText)
        setSuccessToastMessage(statusText)
        setActionMessage(null)
        await loadManagedSkills()
      } catch (err) {
        const raw = err instanceof Error ? err.message : String(err)
        if (raw.startsWith('TARGET_EXISTS|')) {
          const targetPath = raw.split('|')[1] ?? ''
          const shouldOverwrite = window.confirm(
            `检测到该工具目录下已存在「${skill.name}」：\n${targetPath}\n\n是否由 Skills Hub 接管并建立同步（原文件夹将安全移入废纸篓备份）？`,
          )
          if (shouldOverwrite) {
            try {
              setLoading(true)
              if (skillScope === 'project') {
                for (const projectPath of projects) {
                  await invokeTauri('sync_skill_to_tool', {
                    sourcePath: skill.central_path,
                    skillId: skill.id,
                    tool: toolId,
                    name: skill.name,
                    overwrite: true,
                    scope: 'project',
                    projectPath,
                    syncMode: options?.syncMode,
                  })
                }
              } else {
                await invokeTauri('sync_skill_to_tool', {
                  sourcePath: skill.central_path,
                  skillId: skill.id,
                  tool: toolId,
                  name: skill.name,
                  overwrite: true,
                  scope: 'global',
                  syncMode: options?.syncMode,
                })
              }
              const statusText = t('status.syncEnabled')
              setActionMessage(statusText)
              setSuccessToastMessage(statusText)
              setActionMessage(null)
              await loadManagedSkills()
              return
            } catch (retryErr) {
              setError(
                retryErr instanceof Error ? retryErr.message : String(retryErr),
              )
            }
          } else {
            setError(t('errors.targetExistsDetail', { path: targetPath }))
          }
        } else if (raw.startsWith('TOOL_NOT_INSTALLED|')) {
          setError(t('errors.toolNotInstalled'))
        } else if (raw.startsWith('TOOL_NOT_WRITABLE|')) {
          const parts = raw.split('|')
          setError(
            t('errors.toolNotWritable', {
              tool: parts[1] ?? '',
              path: parts[2] ?? '',
            }),
          )
        } else {
          setError(raw)
        }
        await loadManagedSkills()
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
      }
    },
    [
      getSkillProjects,
      getSkillScope,
      loadManagedSkills,
      loading,
      setError,
      setActionMessage,
      setLoading,
      setLoadingStartAt,
      setScopeModalSkill,
      setSuccessToastMessage,
      t,
      tools,
      toolSupportsProjectScope,
    ],
  )

  const handleToggleToolForSkill = useCallback(
    (skill: ManagedSkill, toolId: string) => {
      if (loading) return
      const skillScope = getSkillScope(skill)
      const synced = getToolSyncState(skill, toolId, skillScope) === 'synced'
      // 弹出确认对话框，防止用户误触
      setPendingToolToggle({
        skill,
        toolId,
        action: synced ? 'unsync' : 'sync',
      })
    },
    [getSkillScope, loading],
  )

  const handleSyncToolWithMode = useCallback(
    (skill: ManagedSkill, toolId: string, mode: 'copy' | 'junction') => {
      void runToggleToolForSkill(skill, toolId, {
        targetAction: 'sync',
        syncMode: mode,
      })
    },
    [runToggleToolForSkill],
  )

  const handleUnsyncToolForSkill = useCallback(
    (skill: ManagedSkill, toolId: string) => {
      const skillScope = getSkillScope(skill)
      const currentTarget = skill.targets.find(
        (target) =>
          target.tool === toolId && (target.scope ?? 'global') === skillScope,
      )
      const shared = currentTarget
        ? skill.targets
            .filter(
              (target) =>
                (target.scope ?? 'global') === skillScope &&
                target.target_path === currentTarget.target_path,
            )
            .map((target) => target.tool)
        : sharedToolIdsByToolId[toolId] ?? null
      if (shared && shared.length > 1) {
        setPendingSharedToggle({ skill, toolId, affectedToolIds: shared })
        return
      }
      void runToggleToolForSkill(skill, toolId, { targetAction: 'unsync' })
    },
    [getSkillScope, runToggleToolForSkill, sharedToolIdsByToolId],
  )

  const handleConfirmToolToggle = useCallback(() => {
    if (!pendingToolToggle || loading) return
    const { skill, toolId } = pendingToolToggle
    setPendingToolToggle(null)

    const skillScope = getSkillScope(skill)
    const currentTarget = skill.targets.find(
      (target) =>
        target.tool === toolId && (target.scope ?? 'global') === skillScope,
    )
    const shared = currentTarget
      ? skill.targets
          .filter(
            (target) =>
              (target.scope ?? 'global') === skillScope &&
              target.target_path === currentTarget.target_path,
          )
          .map((target) => target.tool)
      : sharedToolIdsByToolId[toolId] ?? null
    if (shared && shared.length > 1) {
      setPendingSharedToggle({ skill, toolId, affectedToolIds: shared })
      return
    }
    void runToggleToolForSkill(skill, toolId)
  }, [
    getSkillScope,
    loading,
    pendingToolToggle,
    runToggleToolForSkill,
    sharedToolIdsByToolId,
  ])

  const handleSharedCancel = useCallback(() => {
    if (loading) return
    setPendingSharedToggle(null)
  }, [loading])

  const handleSharedConfirm = useCallback(() => {
    if (!pendingSharedToggle) return
    const payload = pendingSharedToggle
    setPendingSharedToggle(null)
    void runToggleToolForSkill(payload.skill, payload.toolId)
  }, [pendingSharedToggle, runToggleToolForSkill])

  const pendingSharedLabels = useMemo(() => {
    if (!pendingSharedToggle) return null
    const toolId = pendingSharedToggle.toolId
    const shared =
      pendingSharedToggle.affectedToolIds ?? sharedToolIdsByToolId[toolId] ?? []
    const others = shared.filter((id) => id !== toolId)
    return {
      toolLabel: toolLabelById[toolId] ?? toolId,
      otherLabels: others.map((id) => toolLabelById[id] ?? id).join(', '),
    }
  }, [pendingSharedToggle, sharedToolIdsByToolId, toolLabelById])

  return {
    pendingSharedToggle,
    setPendingSharedToggle,
    pendingToolToggle,
    setPendingToolToggle,
    pendingSharedLabels,
    runToggleToolForSkill,
    handleToggleToolForSkill,
    handleSyncToolWithMode,
    handleUnsyncToolForSkill,
    handleConfirmToolToggle,
    handleSharedCancel,
    handleSharedConfirm,
  }
}
