import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { invokeTauri } from './useTauriInvoke'
import type { ManagedSkill, TagWithCountDto } from '../components/skills/types'

export interface UseTagOperationsOptions {
  loading: boolean
  setLoading: (loading: boolean) => void
  setLoadingStartAt: (time: number | null) => void
  setActionMessage: (msg: string | null) => void
  setSuccessToastMessage: (msg: string | null) => void
  setError: (err: string | null) => void
  loadManagedSkills: () => Promise<void>
  onTagDeleted?: (deletedTagId: number) => void
  onTagRenamed?: (oldTagId: number, newTagId: number) => void
}

export interface UseTagOperationsReturn {
  tags: TagWithCountDto[]
  setTags: React.Dispatch<React.SetStateAction<TagWithCountDto[]>>
  loadTags: () => Promise<void>
  tagEditorSkill: ManagedSkill | null
  setTagEditorSkill: React.Dispatch<React.SetStateAction<ManagedSkill | null>>
  pendingRenameTag: TagWithCountDto | null
  setPendingRenameTag: React.Dispatch<React.SetStateAction<TagWithCountDto | null>>
  pendingDeleteTag: TagWithCountDto | null
  setPendingDeleteTag: React.Dispatch<React.SetStateAction<TagWithCountDto | null>>
  handleCreateTag: (name: string) => Promise<void>
  handleRenameTag: (tagId: number, name: string) => Promise<void>
  handleCloseRenameTag: () => void
  handleDeleteTag: (tag: TagWithCountDto) => void
  handleCloseDeleteTag: () => void
  handleConfirmDeleteTag: () => Promise<void>
  handleOpenEditTags: (skill: ManagedSkill) => void
  handleCloseEditTags: () => void
  handleSaveSkillTags: (skill: ManagedSkill, tagIds: number[]) => Promise<void>
}

export function useTagOperations({
  loading,
  setLoading,
  setLoadingStartAt,
  setActionMessage,
  setSuccessToastMessage,
  setError,
  loadManagedSkills,
  onTagDeleted,
  onTagRenamed,
}: UseTagOperationsOptions): UseTagOperationsReturn {
  const { t } = useTranslation()
  const [tags, setTags] = useState<TagWithCountDto[]>([])
  const [tagEditorSkill, setTagEditorSkill] = useState<ManagedSkill | null>(null)
  const [pendingRenameTag, setPendingRenameTag] = useState<TagWithCountDto | null>(null)
  const [pendingDeleteTag, setPendingDeleteTag] = useState<TagWithCountDto | null>(null)

  const loadTags = useCallback(async () => {
    try {
      const result = await invokeTauri<TagWithCountDto[]>('get_tags')
      setTags(result)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }, [setError])

  const handleCreateTag = useCallback(
    async (name: string) => {
      try {
        await invokeTauri('create_tag', { name })
        await loadTags()
        setSuccessToastMessage(t('tagCreated'))
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      }
    },
    [loadTags, setError, setSuccessToastMessage, t],
  )

  const handleRenameTag = useCallback(
    async (tagId: number, name: string) => {
      try {
        const renamed = await invokeTauri<{ id: number; name: string }>('rename_tag', {
          tagId,
          name,
        })
        if (onTagRenamed) {
          onTagRenamed(tagId, renamed.id)
        }
        await loadManagedSkills()
        await loadTags()
        setPendingRenameTag(null)
        setSuccessToastMessage(t('tagRenamed'))
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      }
    },
    [loadManagedSkills, loadTags, onTagRenamed, setError, setSuccessToastMessage, t],
  )

  const handleCloseRenameTag = useCallback(() => {
    if (!loading) setPendingRenameTag(null)
  }, [loading])

  const handleDeleteTag = useCallback((tag: TagWithCountDto) => {
    setPendingDeleteTag(tag)
  }, [])

  const handleCloseDeleteTag = useCallback(() => {
    if (!loading) setPendingDeleteTag(null)
  }, [loading])

  const handleConfirmDeleteTag = useCallback(async () => {
    if (!pendingDeleteTag) return
    try {
      setLoading(true)
      setLoadingStartAt(Date.now())
      setActionMessage(t('actions.deletingTag', { name: pendingDeleteTag.name }))
      await invokeTauri('delete_tag', { tagId: pendingDeleteTag.id })
      if (onTagDeleted) {
        onTagDeleted(pendingDeleteTag.id)
      }
      await loadManagedSkills()
      await loadTags()
      setPendingDeleteTag(null)
      setSuccessToastMessage(t('tagDeleted'))
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
      setActionMessage(null)
    }
  }, [
    loadManagedSkills,
    loadTags,
    onTagDeleted,
    pendingDeleteTag,
    setActionMessage,
    setError,
    setLoading,
    setLoadingStartAt,
    setSuccessToastMessage,
    t,
  ])

  const handleOpenEditTags = useCallback((skill: ManagedSkill) => {
    setTagEditorSkill(skill)
  }, [])

  const handleCloseEditTags = useCallback(() => {
    if (!loading) setTagEditorSkill(null)
  }, [loading])

  const handleSaveSkillTags = useCallback(
    async (skill: ManagedSkill, tagIds: number[]) => {
      try {
        setLoading(true)
        setLoadingStartAt(Date.now())
        setActionMessage(t('actions.updatingTags', { name: skill.name }))
        await invokeTauri('set_skill_tags', { skillId: skill.id, tagIds })
        await loadManagedSkills()
        await loadTags()
        setTagEditorSkill(null)
        setSuccessToastMessage(t('tagsUpdated'))
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
      loadTags,
      setActionMessage,
      setError,
      setLoading,
      setLoadingStartAt,
      setSuccessToastMessage,
      t,
    ],
  )

  return {
    tags,
    setTags,
    loadTags,
    tagEditorSkill,
    setTagEditorSkill,
    pendingRenameTag,
    setPendingRenameTag,
    pendingDeleteTag,
    setPendingDeleteTag,
    handleCreateTag,
    handleRenameTag,
    handleCloseRenameTag,
    handleDeleteTag,
    handleCloseDeleteTag,
    handleConfirmDeleteTag,
    handleOpenEditTags,
    handleCloseEditTags,
    handleSaveSkillTags,
  }
}
