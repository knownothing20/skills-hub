import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { isTauri, invokeTauri } from './useTauriInvoke'
import type { OnboardingPlan, ToolOption } from '../components/skills/types'

export interface UseImportPlanProps {
  loading: boolean
  setLoading: (loading: boolean) => void
  setLoadingStartAt: (time: number | null) => void
  setError: (error: string | null) => void
  setActionMessage: (msg: string | null) => void
  setSuccessToastMessage: (msg: string | null) => void
  showActionErrors: (errors: { title: string; message: string }[]) => void
  loadManagedSkills: () => Promise<void>
  tools: ToolOption[]
  syncTargets: Record<string, boolean>
  sharedToolIdsByToolId: Record<string, string[]>
  uniqueToolIdsBySkillsDir: (toolIds: string[]) => string[]
  isInstalled: (toolId: string) => boolean
  defaultSyncMode: 'junction' | 'copy'
}

export function useImportPlan({
  loading,
  setLoading,
  setLoadingStartAt,
  setError,
  setActionMessage,
  setSuccessToastMessage,
  showActionErrors,
  loadManagedSkills,
  tools,
  syncTargets,
  sharedToolIdsByToolId,
  uniqueToolIdsBySkillsDir,
  isInstalled,
  defaultSyncMode,
}: UseImportPlanProps) {
  const { t } = useTranslation()

  const [plan, setPlan] = useState<OnboardingPlan | null>(null)
  const [selected, setSelected] = useState<Record<string, boolean>>({})
  const [variantChoice, setVariantChoice] = useState<Record<string, string>>({})
  const [showImportModal, setShowImportModal] = useState(false)

  const loadPlan = useCallback(
    async (showLoading = true) => {
      if (showLoading) {
        setLoading(true)
        setLoadingStartAt(Date.now())
      }
      setError(null)
      try {
        const result = await invokeTauri<OnboardingPlan>('get_onboarding_plan')
        setPlan(result)
        const defaultSelected: Record<string, boolean> = {}
        const defaultChoice: Record<string, string> = {}
        result.groups.forEach((group) => {
          defaultSelected[group.name] = true
          const first = group.variants[0]
          if (first) {
            defaultChoice[group.name] = first.path
          }
        })
        setSelected(defaultSelected)
        setVariantChoice(defaultChoice)
        return result
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
        return null
      } finally {
        if (showLoading) {
          setLoading(false)
          setLoadingStartAt(null)
        }
      }
    },
    [setError, setLoading, setLoadingStartAt],
  )

  const reloadPlanQuietly = useCallback(() => loadPlan(false), [loadPlan])

  useEffect(() => {
    if (isTauri) {
      // 延迟到完全空闲时在后台检查新发现技能，让首屏渲染获得全部 CPU 资源
      const timer = setTimeout(() => {
        void loadPlan(false)
      }, 4000)
      return () => clearTimeout(timer)
    }
  }, [loadPlan])

  const handleToggleGroup = useCallback((groupName: string, checked: boolean) => {
    setSelected((prev) => ({
      ...prev,
      [groupName]: checked,
    }))
  }, [])

  const handleSelectVariant = useCallback((groupName: string, path: string) => {
    setVariantChoice((prev) => ({
      ...prev,
      [groupName]: path,
    }))
  }, [])

  const handleReviewImport = useCallback(async () => {
    if (plan) {
      setShowImportModal(true)
      return
    }
    const result = await loadPlan()
    if (result) {
      setShowImportModal(true)
    }
  }, [loadPlan, plan])

  const handleCloseImport = useCallback(() => {
    if (!loading) setShowImportModal(false)
  }, [loading])

  const handleImport = useCallback(
    async (mode: 'junction' | 'copy' = defaultSyncMode) => {
      if (!plan) return
      if (!plan.groups.some((group) => selected[group.name])) {
        setError(t('errors.selectAtLeastOneSkill'))
        return
      }
      setLoading(true)
      setLoadingStartAt(Date.now())
      setActionMessage(null)
      setError(null)
      try {
        const collectedErrors: { title: string; message: string }[] = []
        let successCount = 0
        for (const group of plan.groups) {
          if (!selected[group.name]) continue
          const chosenPath = variantChoice[group.name] ?? group.variants[0]?.path
          if (!chosenPath) continue
          const chosenVariant = group.variants.find((v) => v.path === chosenPath)
          const chosenVariantTool = chosenVariant?.tool ?? null
          const chosenFingerprint = chosenVariant?.fingerprint ?? null

          let installResult: {
            skill_id: string
            central_path: string
          }

          try {
            setActionMessage(t('actions.importExisting', { name: group.name }))
            installResult = await invokeTauri<{
              skill_id: string
              central_path: string
            }>('import_existing_skill', {
              sourcePath: chosenPath,
              name: group.name,
            })
            successCount += 1
          } catch (err) {
            collectedErrors.push({
              title: t('errors.importFailedTitle', { name: group.name }),
              message: err instanceof Error ? err.message : String(err),
            })
            continue
          }

          // 仅对该技能实际被检测存在的工具进行接管点亮，原本没有的工具绝不自动复制分发
          const detectedTools = new Set(
            group.variants.flatMap((variant) => {
              const shared = sharedToolIdsByToolId[variant.tool] ?? [variant.tool]
              return [variant.tool, ...shared]
            }),
          )

          const selectedInstalledIds = tools
            .filter(
              (tool) =>
                syncTargets[tool.id] &&
                isInstalled(tool.id) &&
                detectedTools.has(tool.id),
            )
            .map((t) => t.id)
          const targets = uniqueToolIdsBySkillsDir(selectedInstalledIds)
            .map((id) => tools.find((t) => t.id === id))
            .filter(Boolean) as ToolOption[]
          for (const tool of targets) {
            setActionMessage(
              t('actions.syncing', { name: group.name, tool: tool.label }),
            )
            try {
              const sharedToolIds = sharedToolIdsByToolId[tool.id] ?? [tool.id]
              const hasSameContentVariant = Boolean(
                chosenFingerprint &&
                  group.variants.some(
                    (variant) =>
                      sharedToolIds.includes(variant.tool) &&
                      variant.fingerprint === chosenFingerprint,
                  ),
              )
              const isSourceTool = Boolean(
                (chosenVariantTool &&
                  (chosenVariantTool === tool.id ||
                    sharedToolIds.includes(chosenVariantTool))) ||
                  hasSameContentVariant,
              )
              // 新发现技能：对来源软件原有目录固定保持为实体副本，绝不转为软链接
              const targetSyncMode = isSourceTool ? 'copy' : mode
              await invokeTauri('sync_skill_to_tool', {
                sourcePath: installResult.central_path,
                skillId: installResult.skill_id,
                tool: tool.id,
                name: group.name,
                // 自动接管：来源目录或内容一致的已发现目录保持其已有实体副本
                overwrite: isSourceTool,
                overwriteIfSameContent: true,
                syncMode: targetSyncMode,
              })
            } catch (err) {
              const raw = err instanceof Error ? err.message : String(err)
              if (raw.startsWith('TARGET_EXISTS|')) {
                const targetPath = raw.split('|')[1] ?? ''
                collectedErrors.push({
                  title: t('errors.syncFailedTitle', {
                    name: group.name,
                    tool: tool.label,
                  }),
                  message: t('errors.syncTargetExistsMessage', {
                    path: targetPath,
                  }),
                })
              } else {
                collectedErrors.push({
                  title: t('errors.syncFailedTitle', {
                    name: group.name,
                    tool: tool.label,
                  }),
                  message: raw,
                })
              }
            }
          }
        }

        setActionMessage(t('status.importCompleted'))
        setActionMessage(null)
        await loadManagedSkills()
        await loadPlan()
        if (collectedErrors.length > 0) {
          showActionErrors(collectedErrors)
        } else if (successCount > 0) {
          setSuccessToastMessage(t('status.importCompleted'))
        }
        setShowImportModal(false)
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
      }
    },
    [
      defaultSyncMode,
      isInstalled,
      loadManagedSkills,
      loadPlan,
      plan,
      selected,
      setActionMessage,
      setError,
      setLoading,
      setLoadingStartAt,
      setSuccessToastMessage,
      sharedToolIdsByToolId,
      showActionErrors,
      syncTargets,
      t,
      tools,
      uniqueToolIdsBySkillsDir,
      variantChoice,
    ],
  )

  return {
    plan,
    setPlan,
    selected,
    setSelected,
    variantChoice,
    setVariantChoice,
    showImportModal,
    setShowImportModal,
    loadPlan,
    reloadPlanQuietly,
    handleToggleGroup,
    handleSelectVariant,
    handleReviewImport,
    handleCloseImport,
    handleImport,
  }
}
