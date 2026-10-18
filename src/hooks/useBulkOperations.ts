import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { invokeTauri } from './useTauriInvoke'
import { filterUpdateableSkills } from '../components/skills/skillUpdateability'
import { invokeManagedSkillEnabledTransition } from '../components/skills/skillEnabledTransition'
import type { ManagedSkill, ToolOption, UpdateResultDto } from '../components/skills/types'

export interface UseBulkOperationsOptions {
  managedSkills: ManagedSkill[]
  visibleSkills: ManagedSkill[]
  installedTools: ToolOption[]
  installedToolIds: string[]
  installedProjectToolIds: string[]
  toolLabelById: Record<string, string>
  sharedToolIdsByToolId: Record<string, string[]>
  sharedProjectToolIdsByToolId: Record<string, string[]>
  toolSupportsProjectScope: (toolId: string) => boolean
  getSkillScope: (skill: ManagedSkill) => 'global' | 'project'
  getSkillProjects: (skill: ManagedSkill) => string[]
  loading: boolean
  setLoading: (loading: boolean) => void
  setLoadingStartAt: (time: number | null) => void
  setActionMessage: (msg: string | null) => void
  setSuccessToastMessage: (msg: string | null) => void
  setError: (err: string | null) => void
  showActionErrors: (errors: { title: string; message: string }[]) => void
  loadManagedSkills: () => Promise<void>
  loadTags: () => Promise<void>
}

export interface UseBulkOperationsReturn {
  bulkMode: boolean
  setBulkMode: React.Dispatch<React.SetStateAction<boolean>>
  bulkSelectedIds: string[]
  setBulkSelectedIds: React.Dispatch<React.SetStateAction<string[]>>
  bulkSelectedSkills: ManagedSkill[]
  bulkSelectedNames: string[]
  bulkHasDisabledSelected: boolean
  bulkUpdateableSkills: ManagedSkill[]
  bulkShouldEnable: boolean
  allVisibleBulkSelected: boolean
  showBulkSyncModal: boolean
  setShowBulkSyncModal: React.Dispatch<React.SetStateAction<boolean>>
  showBulkDeleteModal: boolean
  setShowBulkDeleteModal: React.Dispatch<React.SetStateAction<boolean>>
  showBulkTagsModal: boolean
  setShowBulkTagsModal: React.Dispatch<React.SetStateAction<boolean>>
  bulkSyncToolIds: string[]
  setBulkSyncToolIds: React.Dispatch<React.SetStateAction<string[]>>
  handleToggleBulkMode: () => void
  handleToggleBulkSelection: (skillId: string) => void
  handleSelectVisibleSkills: () => void
  handleOpenBulkSync: () => void
  handleToggleBulkSyncTool: (toolId: string) => void
  handleCloseBulkSync: () => void
  handleConfirmBulkSync: () => Promise<void>
  handleOpenBulkDelete: () => void
  handleCloseBulkDelete: () => void
  handleConfirmBulkDelete: () => Promise<void>
  handleOpenBulkTags: () => void
  handleCloseBulkTags: () => void
  handleConfirmBulkTags: (addTagIds: number[], removeTagIds: number[]) => Promise<void>
  handleBulkUpdate: () => Promise<void>
  handleToggleBulkEnabled: () => Promise<void>
  resetBulkState: () => void
}

export function useBulkOperations({
  managedSkills,
  visibleSkills,
  installedTools,
  installedToolIds,
  installedProjectToolIds,
  toolLabelById,
  sharedToolIdsByToolId,
  sharedProjectToolIdsByToolId,
  toolSupportsProjectScope,
  getSkillScope,
  getSkillProjects,
  loading,
  setLoading,
  setLoadingStartAt,
  setActionMessage,
  setSuccessToastMessage,
  setError,
  showActionErrors,
  loadManagedSkills,
  loadTags,
}: UseBulkOperationsOptions): UseBulkOperationsReturn {
  const { t } = useTranslation()
  const [bulkMode, setBulkMode] = useState(false)
  const [bulkSelectedIds, setBulkSelectedIds] = useState<string[]>([])
  const [showBulkSyncModal, setShowBulkSyncModal] = useState(false)
  const [showBulkDeleteModal, setShowBulkDeleteModal] = useState(false)
  const [showBulkTagsModal, setShowBulkTagsModal] = useState(false)
  const [bulkSyncToolIds, setBulkSyncToolIds] = useState<string[]>([])

  const bulkSelectedSkills = useMemo(() => {
    const selectedSet = new Set(bulkSelectedIds)
    return managedSkills.filter((skill) => selectedSet.has(skill.id))
  }, [bulkSelectedIds, managedSkills])

  const bulkSelectedNames = useMemo(
    () => bulkSelectedSkills.map((skill) => skill.name),
    [bulkSelectedSkills],
  )

  const bulkHasDisabledSelected = useMemo(
    () => bulkSelectedSkills.some((skill) => skill.enabled === false),
    [bulkSelectedSkills],
  )

  const bulkUpdateableSkills = useMemo(
    () => filterUpdateableSkills(bulkSelectedSkills),
    [bulkSelectedSkills],
  )

  const bulkShouldEnable = useMemo(
    () =>
      bulkSelectedSkills.length > 0 &&
      bulkSelectedSkills.every((skill) => skill.enabled === false),
    [bulkSelectedSkills],
  )

  const allVisibleBulkSelected = useMemo(() => {
    if (visibleSkills.length === 0) return false
    const selectedSet = new Set(bulkSelectedIds)
    return visibleSkills.every((skill) => selectedSet.has(skill.id))
  }, [bulkSelectedIds, visibleSkills])

  useEffect(() => {
    const existingIds = new Set(managedSkills.map((s) => s.id))
    setBulkSelectedIds((current) => current.filter((id) => existingIds.has(id)))
  }, [managedSkills])

  const resetBulkState = useCallback(() => {
    setBulkMode(false)
    setBulkSelectedIds([])
    setShowBulkSyncModal(false)
    setShowBulkDeleteModal(false)
    setShowBulkTagsModal(false)
    setBulkSyncToolIds([])
  }, [])

  const handleToggleBulkMode = useCallback(() => {
    setBulkMode((current) => {
      const next = !current
      if (!next) {
        setBulkSelectedIds([])
        setShowBulkSyncModal(false)
        setShowBulkDeleteModal(false)
        setShowBulkTagsModal(false)
      }
      return next
    })
  }, [])

  const handleToggleBulkSelection = useCallback((skillId: string) => {
    setBulkSelectedIds((current) =>
      current.includes(skillId)
        ? current.filter((id) => id !== skillId)
        : [...current, skillId],
    )
  }, [])

  const handleSelectVisibleSkills = useCallback(() => {
    setBulkMode(true)
    setBulkSelectedIds((current) => {
      const visibleIds = visibleSkills.map((skill) => skill.id)
      const visibleSet = new Set(visibleIds)
      const allVisibleSelected =
        visibleIds.length > 0 && visibleIds.every((id) => current.includes(id))
      if (allVisibleSelected) {
        return current.filter((id) => !visibleSet.has(id))
      }
      return Array.from(new Set([...current, ...visibleIds]))
    })
  }, [visibleSkills])

  const handleOpenBulkSync = useCallback(() => {
    if (bulkSelectedIds.length === 0) return
    if (bulkSelectedSkills.some((skill) => skill.enabled === false)) {
      toast.error(t('bulk.enableBeforeSync'))
      return
    }
    const installedToolSet = new Set(installedToolIds)
    const selectedToolSet = new Set<string>()
    for (const skill of bulkSelectedSkills) {
      const skillScope = getSkillScope(skill)
      for (const target of skill.targets) {
        if ((target.scope ?? 'global') !== skillScope) continue
        if (!installedToolSet.has(target.tool)) continue
        selectedToolSet.add(target.tool)
      }
    }
    setBulkSyncToolIds(
      installedTools
        .map((tool) => tool.id)
        .filter((toolId) => selectedToolSet.has(toolId)),
    )
    setShowBulkSyncModal(true)
  }, [
    bulkSelectedIds.length,
    bulkSelectedSkills,
    getSkillScope,
    installedToolIds,
    installedTools,
    t,
  ])

  const handleToggleBulkSyncTool = useCallback((toolId: string) => {
    setBulkSyncToolIds((current) =>
      current.includes(toolId)
        ? current.filter((id) => id !== toolId)
        : [...current, toolId],
    )
  }, [])

  const handleCloseBulkSync = useCallback(() => {
    if (!loading) setShowBulkSyncModal(false)
  }, [loading])

  const handleOpenBulkDelete = useCallback(() => {
    if (bulkSelectedIds.length === 0) return
    setShowBulkDeleteModal(true)
  }, [bulkSelectedIds.length])

  const handleCloseBulkDelete = useCallback(() => {
    if (!loading) setShowBulkDeleteModal(false)
  }, [loading])

  const handleOpenBulkTags = useCallback(() => {
    if (bulkSelectedIds.length === 0) return
    setShowBulkTagsModal(true)
  }, [bulkSelectedIds.length])

  const handleCloseBulkTags = useCallback(() => {
    if (!loading) setShowBulkTagsModal(false)
  }, [loading])

  const handleConfirmBulkTags = useCallback(
    async (addTagIds: number[], removeTagIds: number[]) => {
      if (bulkSelectedSkills.length === 0) return
      if (addTagIds.length === 0 && removeTagIds.length === 0) return

      const addSet = new Set(addTagIds)
      const removeSet = new Set(removeTagIds)
      const errors: { title: string; message: string }[] = []
      setLoading(true)
      setLoadingStartAt(Date.now())
      setError(null)
      try {
        for (let index = 0; index < bulkSelectedSkills.length; index++) {
          const skill = bulkSelectedSkills[index]
          setActionMessage(
            t('bulk.tagsProgress', {
              current: index + 1,
              total: bulkSelectedSkills.length,
              name: skill.name,
            }),
          )
          const nextTagIds = new Set<number>()
          for (const tag of skill.tags) {
            if (!removeSet.has(tag.id)) nextTagIds.add(tag.id)
          }
          for (const tagId of addSet) nextTagIds.add(tagId)

          try {
            await invokeTauri('set_skill_tags', {
              skillId: skill.id,
              tagIds: Array.from(nextTagIds),
            })
          } catch (err) {
            errors.push({
              title: t('bulk.tagsFailedTitle', { name: skill.name }),
              message: err instanceof Error ? err.message : String(err),
            })
          }
        }

        await loadManagedSkills()
        await loadTags()
        setShowBulkTagsModal(false)
        if (errors.length > 0) {
          showActionErrors(errors)
        } else {
          setBulkSelectedIds([])
          setBulkMode(false)
          setSuccessToastMessage(
            t('bulk.tagsSuccess', { count: bulkSelectedSkills.length }),
          )
        }
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
        setActionMessage(null)
      }
    },
    [
      bulkSelectedSkills,
      loadManagedSkills,
      loadTags,
      setActionMessage,
      setError,
      setLoading,
      setLoadingStartAt,
      setSuccessToastMessage,
      showActionErrors,
      t,
    ],
  )

  const handleBulkUpdate = useCallback(async () => {
    if (bulkSelectedSkills.length === 0) return
    if (bulkUpdateableSkills.length === 0) {
      toast.error(t('bulk.noUpdateSources'))
      return
    }
    const errors: { title: string; message: string }[] = []
    let updated = 0
    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    try {
      for (let index = 0; index < bulkUpdateableSkills.length; index += 1) {
        const skill = bulkUpdateableSkills[index]
        setActionMessage(
          t('bulk.updateProgress', {
            current: index + 1,
            total: bulkUpdateableSkills.length,
            name: skill.name,
          }),
        )
        try {
          await invokeTauri<UpdateResultDto>('update_managed_skill', {
            skillId: skill.id,
          })
          updated += 1
        } catch (err) {
          errors.push({
            title: t('bulk.updateFailedTitle', { name: skill.name }),
            message: err instanceof Error ? err.message : String(err),
          })
        }
      }
      await loadManagedSkills()
      if (errors.length > 0) {
        showActionErrors(errors)
      } else {
        setBulkSelectedIds([])
        setBulkMode(false)
        setSuccessToastMessage(t('bulk.updateSuccess', { count: updated }))
      }
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
      setActionMessage(null)
    }
  }, [
    bulkSelectedSkills.length,
    bulkUpdateableSkills,
    loadManagedSkills,
    setActionMessage,
    setError,
    setLoading,
    setLoadingStartAt,
    setSuccessToastMessage,
    showActionErrors,
    t,
  ])

  const handleToggleBulkEnabled = useCallback(async () => {
    if (bulkSelectedSkills.length === 0) return
    const nextEnabled = bulkShouldEnable
    const affectedSkills = bulkSelectedSkills.filter(
      (skill) => (skill.enabled !== false) !== nextEnabled,
    )
    if (affectedSkills.length === 0) return

    const errors: { title: string; message: string }[] = []
    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    try {
      for (let index = 0; index < affectedSkills.length; index++) {
        const skill = affectedSkills[index]
        setActionMessage(
          nextEnabled
            ? t('bulk.enableProgress', {
                current: index + 1,
                total: affectedSkills.length,
                name: skill.name,
              })
            : t('bulk.disableProgress', {
                current: index + 1,
                total: affectedSkills.length,
                name: skill.name,
              }),
        )
        try {
          await invokeManagedSkillEnabledTransition(
            invokeTauri,
            skill.id,
            nextEnabled,
          )
        } catch (err) {
          errors.push({
            title: nextEnabled
              ? t('bulk.enableFailedTitle', { name: skill.name })
              : t('bulk.disableFailedTitle', { name: skill.name }),
            message: err instanceof Error ? err.message : String(err),
          })
        }
      }
      await loadManagedSkills()
      if (errors.length > 0) {
        showActionErrors(errors)
      } else {
        setBulkSelectedIds([])
        setBulkMode(false)
        setSuccessToastMessage(
          nextEnabled
            ? t('bulk.enableSuccess', { count: affectedSkills.length })
            : t('bulk.disableSuccess', { count: affectedSkills.length }),
        )
      }
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
      setActionMessage(null)
    }
  }, [
    bulkSelectedSkills,
    bulkShouldEnable,
    loadManagedSkills,
    setActionMessage,
    setError,
    setLoading,
    setLoadingStartAt,
    setSuccessToastMessage,
    showActionErrors,
    t,
  ])

  const handleConfirmBulkSync = useCallback(async () => {
    if (bulkSelectedSkills.length === 0) return

    const errors: { title: string; message: string }[] = []
    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    try {
      let completed = 0
      const selectedBaseToolSet = new Set(bulkSyncToolIds)
      const total = Math.max(1, bulkSelectedSkills.length * installedToolIds.length)
      for (const skill of bulkSelectedSkills) {
        const skillScope = getSkillScope(skill)
        const projects = getSkillProjects(skill)
        const sharedByToolId =
          skillScope === 'project'
            ? sharedProjectToolIdsByToolId
            : sharedToolIdsByToolId
        const selectedToolSet = new Set<string>()
        for (const toolId of selectedBaseToolSet) {
          const shared = sharedByToolId[toolId] ?? [toolId]
          for (const id of shared) selectedToolSet.add(id)
        }

        const activeInstalledToolIds =
          skillScope === 'project' ? installedProjectToolIds : installedToolIds

        const targetsToRemove = skill.targets.filter(
          (target) =>
            (target.scope ?? 'global') === skillScope &&
            activeInstalledToolIds.includes(target.tool) &&
            !selectedToolSet.has(target.tool),
        )

        const seenRemoveKeys = new Set<string>()
        for (const target of targetsToRemove) {
          const key = `${target.tool}|${target.scope}|${target.project_path ?? ''}`
          if (seenRemoveKeys.has(key)) continue
          seenRemoveKeys.add(key)
          completed += 1
          const toolLabel = toolLabelById[target.tool] ?? target.tool
          setActionMessage(
            t('bulk.unsyncProgress', {
              current: Math.min(completed, total),
              total,
              name: skill.name,
              tool: toolLabel,
            }),
          )
          try {
            await invokeTauri('unsync_skill_from_tool', {
              skillId: skill.id,
              tool: target.tool,
              scope: skillScope,
              projectPath: target.project_path ?? undefined,
            })
          } catch (err) {
            errors.push({
              title: t('errors.syncFailedTitle', {
                name: skill.name,
                tool: toolLabel,
              }),
              message: err instanceof Error ? err.message : String(err),
            })
          }
        }

        for (const toolId of Array.from(selectedToolSet).filter((id) =>
          activeInstalledToolIds.includes(id),
        )) {
          completed += 1
          const toolLabel = toolLabelById[toolId] ?? toolId
          setActionMessage(
            t('bulk.syncProgress', {
              current: Math.min(completed, total),
              total,
              name: skill.name,
              tool: toolLabel,
            }),
          )
          try {
            if (skillScope === 'project') {
              if (!toolSupportsProjectScope(toolId)) {
                throw new Error(t('projectSync.unsupportedTool', { tool: toolLabel }))
              }
              if (projects.length === 0) {
                throw new Error(t('projectSync.noProjectsForSync'))
              }
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
            } else {
              await invokeTauri('sync_skill_to_tool', {
                sourcePath: skill.central_path,
                skillId: skill.id,
                tool: toolId,
                name: skill.name,
                overwriteIfSameContent: true,
                scope: 'global',
              })
            }
          } catch (err) {
            errors.push({
              title: t('errors.syncFailedTitle', {
                name: skill.name,
                tool: toolLabel,
              }),
              message: err instanceof Error ? err.message : String(err),
            })
          }
        }
      }
      await loadManagedSkills()
      setShowBulkSyncModal(false)
      setBulkSyncToolIds([])
      if (errors.length > 0) {
        showActionErrors(errors)
      } else {
        setBulkSelectedIds([])
        setBulkMode(false)
        setSuccessToastMessage(
          t('bulk.syncSuccess', {
            count: bulkSelectedSkills.length,
            tools: bulkSyncToolIds.length,
          }),
        )
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
      setActionMessage(null)
    }
  }, [
    bulkSelectedSkills,
    bulkSyncToolIds,
    getSkillProjects,
    getSkillScope,
    installedProjectToolIds,
    installedToolIds,
    loadManagedSkills,
    setActionMessage,
    setError,
    setLoading,
    setLoadingStartAt,
    setSuccessToastMessage,
    sharedProjectToolIdsByToolId,
    sharedToolIdsByToolId,
    showActionErrors,
    t,
    toolLabelById,
    toolSupportsProjectScope,
  ])

  const handleConfirmBulkDelete = useCallback(async () => {
    if (bulkSelectedSkills.length === 0) return

    const errors: { title: string; message: string }[] = []
    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    try {
      for (let index = 0; index < bulkSelectedSkills.length; index++) {
        const skill = bulkSelectedSkills[index]
        setActionMessage(
          t('bulk.deleteProgress', {
            current: index + 1,
            total: bulkSelectedSkills.length,
            name: skill.name,
          }),
        )
        try {
          await invokeTauri('delete_managed_skill', { skillId: skill.id })
        } catch (err) {
          errors.push({
            title: skill.name,
            message: err instanceof Error ? err.message : String(err),
          })
        }
      }
      await loadManagedSkills()
      await loadTags()
      setShowBulkDeleteModal(false)
      if (errors.length > 0) {
        showActionErrors(errors)
      } else {
        setBulkSelectedIds([])
        setBulkMode(false)
        setSuccessToastMessage(
          t('bulk.deleteSuccess', { count: bulkSelectedSkills.length }),
        )
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
      setActionMessage(null)
    }
  }, [
    bulkSelectedSkills,
    loadManagedSkills,
    loadTags,
    setActionMessage,
    setError,
    setLoading,
    setLoadingStartAt,
    setSuccessToastMessage,
    showActionErrors,
    t,
  ])

  return {
    bulkMode,
    setBulkMode,
    bulkSelectedIds,
    setBulkSelectedIds,
    bulkSelectedSkills,
    bulkSelectedNames,
    bulkHasDisabledSelected,
    bulkUpdateableSkills,
    bulkShouldEnable,
    allVisibleBulkSelected,
    showBulkSyncModal,
    setShowBulkSyncModal,
    showBulkDeleteModal,
    setShowBulkDeleteModal,
    showBulkTagsModal,
    setShowBulkTagsModal,
    bulkSyncToolIds,
    setBulkSyncToolIds,
    handleToggleBulkMode,
    handleToggleBulkSelection,
    handleSelectVisibleSkills,
    handleOpenBulkSync,
    handleToggleBulkSyncTool,
    handleCloseBulkSync,
    handleConfirmBulkSync,
    handleOpenBulkDelete,
    handleCloseBulkDelete,
    handleConfirmBulkDelete,
    handleOpenBulkTags,
    handleCloseBulkTags,
    handleConfirmBulkTags,
    handleBulkUpdate,
    handleToggleBulkEnabled,
    resetBulkState,
  }
}
