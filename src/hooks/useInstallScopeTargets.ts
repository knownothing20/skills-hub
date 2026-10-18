import {
  useCallback,
  useMemo,
  useRef,
  useState,
  type SetStateAction,
} from 'react'
import { useTranslation } from 'react-i18next'
import { invokeTauri } from './useTauriInvoke'
import {
  buildInstallSyncJobs,
  filterTargetsForScope,
  getAddedProjectPaths,
  isLatestSaveBatch,
  normalizeProjectSharedTargets,
  normalizeProjectPaths,
  resolveProjectPathsUpdate,
  selectInstallToolIds,
  type InstallScope,
} from '../components/skills/installScope'
import type { InstallResultDto, ToolOption } from '../components/skills/types'

export interface UseInstallScopeTargetsProps {
  loading: boolean
  tools: ToolOption[]
  installedTools: ToolOption[]
  installedToolIds: string[]
  toolLabelById: Record<string, string>
  sharedToolIdsByToolId: Record<string, string[]>
  sharedProjectToolIdsByToolId: Record<string, string[]>
  toolSupportsProjectScope: (toolId: string) => boolean
  uniqueToolIdsBySkillsDir: (toolIds: string[]) => string[]
  uniqueToolIdsByProjectSkillsDir: (toolIds: string[]) => string[]
  isInstalled: (id: string) => boolean
  defaultSyncMode: 'junction' | 'copy'
  setActionMessage: (msg: string | null) => void
  setError: (error: string | null) => void
  setRecentProjects: React.Dispatch<React.SetStateAction<string[]>>
}

export function useInstallScopeTargets({
  loading,
  tools,
  installedTools,
  installedToolIds,
  toolLabelById,
  sharedToolIdsByToolId,
  sharedProjectToolIdsByToolId,
  toolSupportsProjectScope,
  uniqueToolIdsBySkillsDir,
  uniqueToolIdsByProjectSkillsDir,
  isInstalled,
  defaultSyncMode,
  setActionMessage,
  setError,
  setRecentProjects,
}: UseInstallScopeTargetsProps) {
  const { t } = useTranslation()

  const [installScope, setInstallScope] = useState<InstallScope>('global')
  const [installProjects, setInstallProjects] = useState<string[]>([])
  const installProjectsRef = useRef<string[]>([])
  const installProjectsSaveSequenceRef = useRef(0)
  const [syncTargets, setSyncTargets] = useState<Record<string, boolean>>({})

  const [pendingSyncTargetChange, setPendingSyncTargetChange] = useState<{
    toolId: string
    checked: boolean
    affected: string[]
    shared: string[]
  } | null>(null)

  const resetInstallScope = useCallback(() => {
    setInstallScope('global')
    installProjectsRef.current = []
    installProjectsSaveSequenceRef.current += 1
    setInstallProjects([])
  }, [])

  const syncInstalledSkill = useCallback(
    async (
      created: InstallResultDto,
    ): Promise<{ title: string; message: string }[]> => {
      const toolIds = selectInstallToolIds(
        tools,
        syncTargets,
        installedToolIds,
        installScope,
        uniqueToolIdsBySkillsDir,
        uniqueToolIdsByProjectSkillsDir,
      )
      const jobs = buildInstallSyncJobs(
        toolIds,
        installScope,
        installProjects,
      )

      if (jobs.length === 0) {
        return [
          {
            title: t('errors.unsyncedTitle', { name: created.name }),
            message:
              installScope === 'project' &&
              normalizeProjectPaths(installProjects).length === 0
                ? t('projectSync.projectRequired')
                : t('errors.noSyncTargets'),
          },
        ]
      }

      const collectedErrors: { title: string; message: string }[] = []
      for (let index = 0; index < jobs.length; index++) {
        const job = jobs[index]
        const toolLabel = toolLabelById[job.toolId] ?? job.toolId
        setActionMessage(
          t('actions.syncStep', {
            index: index + 1,
            total: jobs.length,
            name: created.name,
            tool: toolLabel,
          }),
        )
        try {
          await invokeTauri('sync_skill_to_tool', {
            sourcePath: created.central_path,
            skillId: created.skill_id,
            tool: job.toolId,
            name: created.name,
            scope: job.scope,
            ...(job.scope === 'project' && job.projectPath
              ? { projectPath: job.projectPath }
              : {}),
            overwriteIfSameContent: true,
            syncMode: defaultSyncMode,
          })
        } catch (err) {
          collectedErrors.push({
            title: t('errors.syncFailedTitle', {
              name: created.name,
              tool: toolLabel,
            }),
            message: err instanceof Error ? err.message : String(err),
          })
        }
      }

      return collectedErrors
    },
    [
      defaultSyncMode,
      installProjects,
      installScope,
      installedToolIds,
      setActionMessage,
      syncTargets,
      t,
      toolLabelById,
      tools,
      uniqueToolIdsByProjectSkillsDir,
      uniqueToolIdsBySkillsDir,
    ],
  )

  const applySyncTargetChange = useCallback(
    (checked: boolean, affected: string[], shared: string[]) => {
      setSyncTargets((prev) => {
        const next = { ...prev }
        for (const id of affected) next[id] = checked
        if (installScope === 'project') {
          for (const id of shared) {
            if (!toolSupportsProjectScope(id)) next[id] = false
          }
        }
        return next
      })
    },
    [installScope, toolSupportsProjectScope],
  )

  const handleSyncTargetChange = useCallback(
    (toolId: string, checked: boolean) => {
      const sharedByToolId =
        installScope === 'project'
          ? sharedProjectToolIdsByToolId
          : sharedToolIdsByToolId
      const shared = sharedByToolId[toolId] ?? [toolId]
      const affected =
        installScope === 'project'
          ? shared.filter(
              (id) => isInstalled(id) && toolSupportsProjectScope(id),
            )
          : shared
      if (affected.length > 1) {
        setPendingSyncTargetChange({ toolId, checked, affected, shared })
        return
      }
      applySyncTargetChange(checked, affected, shared)
    },
    [
      applySyncTargetChange,
      installScope,
      isInstalled,
      sharedProjectToolIdsByToolId,
      sharedToolIdsByToolId,
      toolSupportsProjectScope,
    ],
  )

  const handleSyncTargetChangeCancel = useCallback(() => {
    if (!loading) setPendingSyncTargetChange(null)
  }, [loading])

  const handleSyncTargetChangeConfirm = useCallback(() => {
    if (!pendingSyncTargetChange) return
    const { checked, affected, shared } = pendingSyncTargetChange
    setPendingSyncTargetChange(null)
    applySyncTargetChange(checked, affected, shared)
  }, [applySyncTargetChange, pendingSyncTargetChange])

  const pendingSyncTargetLabels = useMemo(() => {
    if (!pendingSyncTargetChange) return null
    const others = pendingSyncTargetChange.affected.filter(
      (id) => id !== pendingSyncTargetChange.toolId,
    )
    return {
      toolLabel:
        toolLabelById[pendingSyncTargetChange.toolId] ??
        pendingSyncTargetChange.toolId,
      otherLabels: others.map((id) => toolLabelById[id] ?? id).join(', '),
    }
  }, [pendingSyncTargetChange, toolLabelById])

  const handleInstallScopeChange = useCallback(
    (nextScope: InstallScope) => {
      setInstallScope(nextScope)
      if (nextScope === 'project') {
        setSyncTargets((current) => {
          const filtered = filterTargetsForScope(
            current,
            installedTools,
            nextScope,
          )
          return normalizeProjectSharedTargets(
            filtered,
            installedTools,
            sharedProjectToolIdsByToolId,
          )
        })
      }
    },
    [installedTools, sharedProjectToolIdsByToolId],
  )

  const handleInstallProjectsChange = useCallback(
    async (nextProjects: SetStateAction<string[]>) => {
      const normalizedProjects = resolveProjectPathsUpdate(
        installProjectsRef.current,
        nextProjects,
      )
      const addedProjects = getAddedProjectPaths(
        installProjectsRef.current,
        normalizedProjects,
      )
      installProjectsRef.current = normalizedProjects
      setInstallProjects(normalizedProjects)
      if (addedProjects.length === 0) return

      const saveSequence = installProjectsSaveSequenceRef.current + 1
      installProjectsSaveSequenceRef.current = saveSequence
      try {
        let savedProjects: string[] | null = null
        for (const projectPath of addedProjects) {
          savedProjects = await invokeTauri<string[]>('save_recent_project', {
            projectPath,
          })
        }
        if (
          savedProjects &&
          isLatestSaveBatch(saveSequence, installProjectsSaveSequenceRef.current)
        ) {
          setRecentProjects(savedProjects)
        }
      } catch (err) {
        if (
          isLatestSaveBatch(
            saveSequence,
            installProjectsSaveSequenceRef.current,
          )
        ) {
          setError(err instanceof Error ? err.message : String(err))
        }
      }
    },
    [setError, setRecentProjects],
  )

  return {
    installScope,
    setInstallScope,
    installProjects,
    setInstallProjects,
    syncTargets,
    setSyncTargets,
    pendingSyncTargetChange,
    setPendingSyncTargetChange,
    pendingSyncTargetLabels,
    resetInstallScope,
    syncInstalledSkill,
    applySyncTargetChange,
    handleSyncTargetChange,
    handleSyncTargetChangeCancel,
    handleSyncTargetChangeConfirm,
    handleInstallScopeChange,
    handleInstallProjectsChange,
  }
}
