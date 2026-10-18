import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { invokeTauri } from './useTauriInvoke'
import { invokeManagedSkillEnabledTransition } from '../components/skills/skillEnabledTransition'
import type { ManagedSkill, UpdateResultDto } from '../components/skills/types'
import type { SkillScopeState } from './useSkillScope'

export interface UseSkillLifecycleProps {
  managedSkills: ManagedSkill[]
  loading: boolean
  setLoading: (loading: boolean) => void
  setLoadingStartAt: (time: number | null) => void
  setError: (error: string | null) => void
  setActionMessage: (msg: string | null) => void
  setSuccessToastMessage: (msg: string | null) => void
  setSkillScopeState: React.Dispatch<React.SetStateAction<SkillScopeState>>
  loadManagedSkills: () => Promise<void>
  loadTags: () => Promise<void>
}

export function useSkillLifecycle({
  managedSkills,
  loading,
  setLoading,
  setLoadingStartAt,
  setError,
  setActionMessage,
  setSuccessToastMessage,
  setSkillScopeState,
  loadManagedSkills,
  loadTags,
}: UseSkillLifecycleProps) {
  const { t } = useTranslation()

  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null)

  const pendingDeleteSkill = useMemo(
    () => managedSkills.find((skill) => skill.id === pendingDeleteId) ?? null,
    [managedSkills, pendingDeleteId],
  )

  const handleDeletePrompt = useCallback((skillId: string) => {
    setPendingDeleteId(skillId)
  }, [])

  const handleCloseDelete = useCallback(() => {
    if (!loading) setPendingDeleteId(null)
  }, [loading])

  const handleDeleteManaged = useCallback(
    async (skill: ManagedSkill) => {
      setLoading(true)
      setLoadingStartAt(Date.now())
      setActionMessage(t('actions.removing', { name: skill.name }))
      setError(null)
      try {
        await invokeTauri('delete_managed_skill', { skillId: skill.id })
        setActionMessage(t('status.skillRemoved'))
        setSuccessToastMessage(t('status.skillRemoved'))
        setActionMessage(null)
        setSkillScopeState((prev) => {
          const next = { ...prev }
          delete next[skill.id]
          return next
        })
        await loadManagedSkills()
        await loadTags()
        setPendingDeleteId(null)
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
      }
    },
    [
      loadManagedSkills,
      loadTags,
      setActionMessage,
      setError,
      setLoading,
      setLoadingStartAt,
      setSkillScopeState,
      setSuccessToastMessage,
      t,
    ],
  )

  const handleToggleSkillEnabled = useCallback(
    async (skill: ManagedSkill) => {
      const nextEnabled = skill.enabled === false
      setLoading(true)
      setLoadingStartAt(Date.now())
      setError(null)
      setActionMessage(
        nextEnabled
          ? t('bulk.enableProgress', {
              current: 1,
              total: 1,
              name: skill.name,
            })
          : t('bulk.disableProgress', {
              current: 1,
              total: 1,
              name: skill.name,
            }),
      )
      try {
        await invokeManagedSkillEnabledTransition(
          invokeTauri,
          skill.id,
          nextEnabled,
        )
        await loadManagedSkills()
        setSuccessToastMessage(
          nextEnabled
            ? t('skillEnabled', { name: skill.name })
            : t('skillDisabled', { name: skill.name }),
        )
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
        setActionMessage(null)
      }
    },
    [
      loadManagedSkills,
      setActionMessage,
      setError,
      setLoading,
      setLoadingStartAt,
      setSuccessToastMessage,
      t,
    ],
  )

  const handleUpdateManaged = useCallback(
    async (skill: ManagedSkill) => {
      setLoading(true)
      setLoadingStartAt(Date.now())
      setError(null)
      try {
        setActionMessage(t('actions.updating', { name: skill.name }))
        await invokeTauri<UpdateResultDto>('update_managed_skill', {
          skillId: skill.id,
        })
        const updatedText = t('status.updated', { name: skill.name })
        setActionMessage(updatedText)
        setSuccessToastMessage(updatedText)
        setActionMessage(null)
        await loadManagedSkills()
      } catch (err) {
        const raw = err instanceof Error ? err.message : String(err)
        setError(raw)
        await loadManagedSkills()
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
      }
    },
    [
      loadManagedSkills,
      setActionMessage,
      setError,
      setLoading,
      setLoadingStartAt,
      setSuccessToastMessage,
      t,
    ],
  )

  const handleUpdateSkill = useCallback(
    (skill: ManagedSkill) => {
      void handleUpdateManaged(skill)
    },
    [handleUpdateManaged],
  )

  return {
    pendingDeleteId,
    setPendingDeleteId,
    pendingDeleteSkill,
    handleDeletePrompt,
    handleCloseDelete,
    handleDeleteManaged,
    handleToggleSkillEnabled,
    handleUpdateManaged,
    handleUpdateSkill,
  }
}
