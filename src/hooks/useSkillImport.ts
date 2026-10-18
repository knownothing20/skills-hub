import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { invokeTauri } from './useTauriInvoke'
import type {
  GitSkillCandidate,
  InstallResultDto,
  LocalSkillCandidate,
} from '../components/skills/types'

export interface UseSkillImportOptions {
  isSkillNameTaken: (name: string) => boolean
  syncInstalledSkill: (created: InstallResultDto) => Promise<{ title: string; message: string }[]>
  resetInstallScope: () => void
  loadManagedSkills: () => Promise<void>
  loadTags: () => Promise<void>
  loading: boolean
  setLoading: (loading: boolean) => void
  setLoadingStartAt: (time: number | null) => void
  setActionMessage: (msg: string | null) => void
  setSuccessToastMessage: (msg: string | null) => void
  setError: (err: string | null) => void
  showActionErrors: (errors: { title: string; message: string }[]) => void
}

export interface UseSkillImportReturn {
  showAddModal: boolean
  setShowAddModal: React.Dispatch<React.SetStateAction<boolean>>
  addModalTab: 'local' | 'git'
  setAddModalTab: React.Dispatch<React.SetStateAction<'local' | 'git'>>
  addModalTagIds: number[]
  setAddModalTagIds: React.Dispatch<React.SetStateAction<number[]>>
  localPath: string
  setLocalPath: React.Dispatch<React.SetStateAction<string>>
  localName: string
  setLocalName: React.Dispatch<React.SetStateAction<string>>
  gitUrl: string
  setGitUrl: React.Dispatch<React.SetStateAction<string>>
  gitName: string
  setGitName: React.Dispatch<React.SetStateAction<string>>
  showGitPickModal: boolean
  setShowGitPickModal: React.Dispatch<React.SetStateAction<boolean>>
  gitCandidates: GitSkillCandidate[]
  setGitCandidates: React.Dispatch<React.SetStateAction<GitSkillCandidate[]>>
  gitCandidatesRepoUrl: string
  setGitCandidatesRepoUrl: React.Dispatch<React.SetStateAction<string>>
  gitCandidateSelected: Record<string, boolean>
  setGitCandidateSelected: React.Dispatch<React.SetStateAction<Record<string, boolean>>>
  showLocalPickModal: boolean
  setShowLocalPickModal: React.Dispatch<React.SetStateAction<boolean>>
  localCandidates: LocalSkillCandidate[]
  setLocalCandidates: React.Dispatch<React.SetStateAction<LocalSkillCandidate[]>>
  localCandidatesBasePath: string
  setLocalCandidatesBasePath: React.Dispatch<React.SetStateAction<string>>
  localCandidateSelected: Record<string, boolean>
  setLocalCandidateSelected: React.Dispatch<React.SetStateAction<Record<string, boolean>>>
  autoSelectSkillName: string | null
  setAutoSelectSkillName: React.Dispatch<React.SetStateAction<string | null>>
  handleCloseAdd: () => void
  handleCloseGitPick: () => void
  handleCancelGitPick: () => void
  handleCloseLocalPick: () => void
  handleCancelLocalPick: () => void
  handleToggleLocalCandidate: (subpath: string) => void
  handleToggleGitCandidate: (subpath: string) => void
  handleSelectAllLocalCandidates: (selected: boolean) => void
  handleSelectAllGitCandidates: (selected: boolean) => void
  handleCreateLocal: () => Promise<void>
  handleCreateGit: () => Promise<void>
  handleInstallSelectedLocalCandidates: () => Promise<void>
  handleInstallSelectedCandidates: () => Promise<void>
}

export function useSkillImport({
  isSkillNameTaken,
  syncInstalledSkill,
  resetInstallScope,
  loadManagedSkills,
  loadTags,
  loading,
  setLoading,
  setLoadingStartAt,
  setActionMessage,
  setSuccessToastMessage,
  setError,
  showActionErrors,
}: UseSkillImportOptions): UseSkillImportReturn {
  const { t } = useTranslation()

  const [showAddModal, setShowAddModal] = useState(false)
  const [addModalTab, setAddModalTab] = useState<'local' | 'git'>('git')
  const [addModalTagIds, setAddModalTagIds] = useState<number[]>([])

  const [localPath, setLocalPath] = useState('')
  const [localName, setLocalName] = useState('')
  const [showLocalPickModal, setShowLocalPickModal] = useState(false)
  const [localCandidates, setLocalCandidates] = useState<LocalSkillCandidate[]>([])
  const [localCandidatesBasePath, setLocalCandidatesBasePath] = useState('')
  const [localCandidateSelected, setLocalCandidateSelected] = useState<
    Record<string, boolean>
  >({})

  const [gitUrl, setGitUrl] = useState('')
  const [gitName, setGitName] = useState('')
  const [showGitPickModal, setShowGitPickModal] = useState(false)
  const [gitCandidates, setGitCandidates] = useState<GitSkillCandidate[]>([])
  const [gitCandidatesRepoUrl, setGitCandidatesRepoUrl] = useState<string>('')
  const [gitCandidateSelected, setGitCandidateSelected] = useState<
    Record<string, boolean>
  >({})
  const [autoSelectSkillName, setAutoSelectSkillName] = useState<string | null>(null)

  const applySelectedAddModalTags = useCallback(
    async (skillId: string, skillName: string) => {
      if (addModalTagIds.length === 0) return
      try {
        await invokeTauri('set_skill_tags', {
          skillId,
          tagIds: addModalTagIds,
        })
      } catch {
        toast.error(t('tagsApplyFailed', { name: skillName }))
      }
    },
    [addModalTagIds, t],
  )

  const handleCloseAdd = useCallback(() => {
    if (!loading) {
      setShowAddModal(false)
      setAddModalTagIds([])
      resetInstallScope()
    }
  }, [loading, resetInstallScope])

  const handleCloseGitPick = useCallback(() => {
    if (!loading) setShowGitPickModal(false)
  }, [loading])

  const handleCancelGitPick = useCallback(() => {
    if (loading) return
    setShowGitPickModal(false)
    setGitCandidates([])
    setGitCandidateSelected({})
    setGitCandidatesRepoUrl('')
    setShowAddModal(true)
  }, [loading])

  const handleCloseLocalPick = useCallback(() => {
    if (!loading) setShowLocalPickModal(false)
  }, [loading])

  const handleCancelLocalPick = useCallback(() => {
    if (loading) return
    setShowLocalPickModal(false)
    setLocalCandidates([])
    setLocalCandidateSelected({})
    setLocalCandidatesBasePath('')
    setShowAddModal(true)
  }, [loading])

  const handleToggleLocalCandidate = useCallback((subpath: string) => {
    setLocalCandidateSelected((prev) => ({
      ...prev,
      [subpath]: !prev[subpath],
    }))
  }, [])

  const handleToggleGitCandidate = useCallback((subpath: string) => {
    setGitCandidateSelected((prev) => ({
      ...prev,
      [subpath]: !prev[subpath],
    }))
  }, [])

  const handleSelectAllLocalCandidates = useCallback(
    (selected: boolean) => {
      setLocalCandidateSelected(
        Object.fromEntries(
          localCandidates.map((c) => [c.subpath, c.valid && selected]),
        ),
      )
    },
    [localCandidates],
  )

  const handleSelectAllGitCandidates = useCallback(
    (selected: boolean) => {
      setGitCandidateSelected(
        Object.fromEntries(gitCandidates.map((c) => [c.subpath, selected])),
      )
    },
    [gitCandidates],
  )

  const handleCreateLocal = useCallback(async () => {
    if (!localPath.trim()) {
      setError(t('errors.requireLocalPath'))
      return
    }
    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    setActionMessage(t('actions.creatingLocalSkill'))
    try {
      const basePath = localPath.trim()
      const candidates = await invokeTauri<LocalSkillCandidate[]>(
        'list_local_skills_cmd',
        { basePath },
      )
      if (candidates.length === 0) {
        throw new Error(t('errors.noSkillsFoundLocal'))
      }
      if (candidates.length === 1 && candidates[0].valid) {
        const desiredName = localName.trim() || candidates[0].name
        if (isSkillNameTaken(desiredName)) {
          setError(t('errors.skillAlreadyExists', { name: desiredName }))
          return
        }
        const created = await invokeTauri<InstallResultDto>(
          'install_local_selection',
          {
            basePath,
            subpath: candidates[0].subpath,
            name: localName.trim() || undefined,
          },
        )
        await applySelectedAddModalTags(created.skill_id, created.name)
        const syncErrors = await syncInstalledSkill(created)
        if (syncErrors.length > 0) showActionErrors(syncErrors)
        setLocalPath('')
        setLocalName('')
        setActionMessage(t('status.localSkillCreated'))
        setSuccessToastMessage(t('status.localSkillCreated'))
        setActionMessage(null)
        resetInstallScope()
        setShowAddModal(false)
        await loadManagedSkills()
        await loadTags()
      } else {
        setLocalCandidatesBasePath(basePath)
        setLocalCandidates(candidates)
        setLocalCandidateSelected(
          Object.fromEntries(candidates.map((c) => [c.subpath, c.valid])),
        )
        setShowLocalPickModal(true)
        setActionMessage(null)
        setLoading(false)
        setLoadingStartAt(null)
        return
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
    }
  }, [
    applySelectedAddModalTags,
    isSkillNameTaken,
    loadManagedSkills,
    loadTags,
    localName,
    localPath,
    resetInstallScope,
    setActionMessage,
    setError,
    setLoading,
    setLoadingStartAt,
    setSuccessToastMessage,
    showActionErrors,
    syncInstalledSkill,
    t,
  ])

  const handleCreateGit = useCallback(async () => {
    if (!gitUrl.trim()) {
      setError(t('errors.requireGitUrl'))
      return
    }
    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    setActionMessage(t('actions.creatingGitSkill'))
    try {
      const url = gitUrl.trim()
      const isFolderUrl = url.includes('/tree/') || url.includes('/blob/')

      if (isFolderUrl) {
        const candidates = await invokeTauri<GitSkillCandidate[]>(
          'list_git_skills_cmd',
          { repoUrl: url },
        )
        if (candidates.length === 0) {
          throw new Error(t('errors.noSkillsFoundWithHint'))
        }
        if (candidates.length > 1) {
          setGitCandidatesRepoUrl(url)
          setGitCandidates(candidates)
          setGitCandidateSelected(
            Object.fromEntries(candidates.map((c) => [c.subpath, true])),
          )
          setShowGitPickModal(true)
          setActionMessage(null)
          setLoading(false)
          setLoadingStartAt(null)
          return
        }
        if (isSkillNameTaken(candidates[0].name)) {
          setError(t('errors.skillAlreadyExists', { name: candidates[0].name }))
          return
        }
        const created = await invokeTauri<InstallResultDto>(
          'install_git_selection',
          {
            repoUrl: url,
            subpath: candidates[0].subpath,
            name: gitName.trim() || undefined,
          },
        )
        await applySelectedAddModalTags(created.skill_id, created.name)
        const syncErrors = await syncInstalledSkill(created)
        if (syncErrors.length > 0) showActionErrors(syncErrors)
      } else {
        const candidates = await invokeTauri<GitSkillCandidate[]>(
          'list_git_skills_cmd',
          { repoUrl: url },
        )
        if (candidates.length === 0) {
          throw new Error(t('errors.noSkillsFoundWithHint'))
        }
        if (candidates.length === 1) {
          if (isSkillNameTaken(candidates[0].name)) {
            setError(t('errors.skillAlreadyExists', { name: candidates[0].name }))
            return
          }
          const created = await invokeTauri<InstallResultDto>(
            'install_git_selection',
            {
              repoUrl: url,
              subpath: candidates[0].subpath,
              name: gitName.trim() || undefined,
            },
          )
          await applySelectedAddModalTags(created.skill_id, created.name)
          const syncErrors = await syncInstalledSkill(created)
          if (syncErrors.length > 0) showActionErrors(syncErrors)
        } else if (autoSelectSkillName) {
          const target = autoSelectSkillName.toLowerCase()
          const containMatches = candidates.filter((c) => {
            const n = c.name.toLowerCase()
            return target.includes(n) || n.includes(target)
          })
          const match =
            candidates.find((c) => c.name.toLowerCase() === target) ??
            (containMatches.length === 1 ? containMatches[0] : undefined)
          setAutoSelectSkillName(null)
          if (match) {
            if (isSkillNameTaken(match.name)) {
              setError(t('errors.skillAlreadyExists', { name: match.name }))
              return
            }
            const created = await invokeTauri<InstallResultDto>(
              'install_git_selection',
              {
                repoUrl: url,
                subpath: match.subpath,
                name: gitName.trim() || undefined,
              },
            )
            await applySelectedAddModalTags(created.skill_id, created.name)
            const syncErrors = await syncInstalledSkill(created)
            if (syncErrors.length > 0) showActionErrors(syncErrors)
          } else {
            setGitCandidatesRepoUrl(url)
            setGitCandidates(candidates)
            setGitCandidateSelected(
              Object.fromEntries(candidates.map((c) => [c.subpath, true])),
            )
            setShowGitPickModal(true)
            setActionMessage(null)
            setLoading(false)
            setLoadingStartAt(null)
            return
          }
        } else {
          setGitCandidatesRepoUrl(url)
          setGitCandidates(candidates)
          setGitCandidateSelected(
            Object.fromEntries(candidates.map((c) => [c.subpath, true])),
          )
          setShowGitPickModal(true)
          setActionMessage(null)
          setLoading(false)
          setLoadingStartAt(null)
          return
        }
      }
      setGitUrl('')
      setGitName('')
      setActionMessage(t('status.gitSkillCreated'))
      setSuccessToastMessage(t('status.gitSkillCreated'))
      setActionMessage(null)
      resetInstallScope()
      setShowAddModal(false)
      await loadManagedSkills()
      await loadTags()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
    }
  }, [
    applySelectedAddModalTags,
    autoSelectSkillName,
    gitName,
    gitUrl,
    isSkillNameTaken,
    loadManagedSkills,
    loadTags,
    resetInstallScope,
    setActionMessage,
    setError,
    setLoading,
    setLoadingStartAt,
    setSuccessToastMessage,
    showActionErrors,
    syncInstalledSkill,
    t,
  ])

  const handleInstallSelectedLocalCandidates = useCallback(async () => {
    const selected = localCandidates.filter(
      (c) => c.valid && localCandidateSelected[c.subpath],
    )
    if (selected.length === 0) {
      setError(t('errors.selectAtLeastOneSkill'))
      return
    }
    if (selected.length > 1 && localName.trim()) {
      setError(t('errors.multiSelectNoCustomName'))
      return
    }
    if (selected.length > 1) {
      const seen = new Set<string>()
      const dup = selected.find((c) => {
        if (seen.has(c.name)) return true
        seen.add(c.name)
        return false
      })
      if (dup) {
        setError(t('errors.duplicateSelectedSkills', { name: dup.name }))
        return
      }
    }
    const desiredName =
      selected.length === 1 && localName.trim()
        ? localName.trim()
        : selected[0].name
    if (selected.length === 1 && isSkillNameTaken(desiredName)) {
      setError(t('errors.skillAlreadyExists', { name: desiredName }))
      return
    }
    const duplicated = selected.find((c) => isSkillNameTaken(c.name))
    if (selected.length > 1 && duplicated) {
      setError(t('errors.skillAlreadyExists', { name: duplicated.name }))
      return
    }

    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    try {
      const collectedErrors: { title: string; message: string }[] = []
      for (let i = 0; i < selected.length; i++) {
        const candidate = selected[i]
        setActionMessage(
          t('actions.importStep', {
            index: i + 1,
            total: selected.length,
            name: candidate.name,
          }),
        )
        try {
          const created = await invokeTauri<InstallResultDto>(
            'install_local_selection',
            {
              basePath: localCandidatesBasePath,
              subpath: candidate.subpath,
              name: localName.trim() || undefined,
            },
          )
          await applySelectedAddModalTags(created.skill_id, created.name)
          const syncErrors = await syncInstalledSkill(created)
          collectedErrors.push(...syncErrors)
        } catch (err) {
          const raw = err instanceof Error ? err.message : String(err)
          collectedErrors.push({
            title: t('errors.importFailedTitle', { name: candidate.name }),
            message: raw,
          })
        }
      }

      setShowLocalPickModal(false)
      setLocalCandidates([])
      setLocalCandidateSelected({})
      setLocalCandidatesBasePath('')
      setLocalPath('')
      setLocalName('')
      setActionMessage(t('status.selectedSkillsInstalled'))
      setSuccessToastMessage(t('status.selectedSkillsInstalled'))
      setActionMessage(null)
      resetInstallScope()
      setShowAddModal(false)
      await loadManagedSkills()
      await loadTags()
      if (collectedErrors.length > 0) showActionErrors(collectedErrors)
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
    }
  }, [
    applySelectedAddModalTags,
    isSkillNameTaken,
    loadManagedSkills,
    loadTags,
    localCandidateSelected,
    localCandidates,
    localCandidatesBasePath,
    localName,
    resetInstallScope,
    setActionMessage,
    setError,
    setLoading,
    setLoadingStartAt,
    setSuccessToastMessage,
    showActionErrors,
    syncInstalledSkill,
    t,
  ])

  const handleInstallSelectedCandidates = useCallback(async () => {
    const selected = gitCandidates.filter((c) => gitCandidateSelected[c.subpath])
    if (selected.length === 0) {
      setError(t('errors.selectAtLeastOneSkill'))
      return
    }
    const duplicated = selected.find((c) => isSkillNameTaken(c.name))
    if (duplicated) {
      setError(t('errors.skillAlreadyExists', { name: duplicated.name }))
      return
    }
    if (selected.length > 1 && gitName.trim()) {
      setError(t('errors.multiSelectNoCustomName'))
      return
    }

    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    try {
      const collectedErrors: { title: string; message: string }[] = []
      for (let i = 0; i < selected.length; i++) {
        const candidate = selected[i]
        setActionMessage(
          t('actions.importStep', {
            index: i + 1,
            total: selected.length,
            name: candidate.name,
          }),
        )
        try {
          const created = await invokeTauri<InstallResultDto>(
            'install_git_selection',
            {
              repoUrl: gitCandidatesRepoUrl,
              subpath: candidate.subpath,
              name: gitName.trim() || undefined,
            },
          )
          await applySelectedAddModalTags(created.skill_id, created.name)
          const syncErrors = await syncInstalledSkill(created)
          collectedErrors.push(...syncErrors)
        } catch (err) {
          const raw = err instanceof Error ? err.message : String(err)
          collectedErrors.push({
            title: t('errors.importFailedTitle', { name: candidate.name }),
            message: raw,
          })
        }
      }

      setShowGitPickModal(false)
      setGitCandidates([])
      setGitCandidateSelected({})
      setGitCandidatesRepoUrl('')
      setGitUrl('')
      setGitName('')
      setActionMessage(t('status.selectedSkillsInstalled'))
      setSuccessToastMessage(t('status.selectedSkillsInstalled'))
      setActionMessage(null)
      setShowGitPickModal(false)
      setGitCandidates([])
      setGitCandidateSelected({})
      setGitCandidatesRepoUrl('')
      setGitUrl('')
      setGitName('')
      resetInstallScope()
      setShowAddModal(false)
      await loadManagedSkills()
      await loadTags()
      if (collectedErrors.length > 0) showActionErrors(collectedErrors)
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
    }
  }, [
    applySelectedAddModalTags,
    gitCandidateSelected,
    gitCandidates,
    gitCandidatesRepoUrl,
    gitName,
    isSkillNameTaken,
    loadManagedSkills,
    loadTags,
    resetInstallScope,
    setActionMessage,
    setError,
    setLoading,
    setLoadingStartAt,
    setSuccessToastMessage,
    showActionErrors,
    syncInstalledSkill,
    t,
  ])

  return {
    showAddModal,
    setShowAddModal,
    addModalTab,
    setAddModalTab,
    addModalTagIds,
    setAddModalTagIds,
    localPath,
    setLocalPath,
    localName,
    setLocalName,
    gitUrl,
    setGitUrl,
    gitName,
    setGitName,
    showGitPickModal,
    setShowGitPickModal,
    gitCandidates,
    setGitCandidates,
    gitCandidatesRepoUrl,
    setGitCandidatesRepoUrl,
    gitCandidateSelected,
    setGitCandidateSelected,
    showLocalPickModal,
    setShowLocalPickModal,
    localCandidates,
    setLocalCandidates,
    localCandidatesBasePath,
    setLocalCandidatesBasePath,
    localCandidateSelected,
    setLocalCandidateSelected,
    autoSelectSkillName,
    setAutoSelectSkillName,
    handleCloseAdd,
    handleCloseGitPick,
    handleCancelGitPick,
    handleCloseLocalPick,
    handleCancelLocalPick,
    handleToggleLocalCandidate,
    handleToggleGitCandidate,
    handleSelectAllLocalCandidates,
    handleSelectAllGitCandidates,
    handleCreateLocal,
    handleCreateGit,
    handleInstallSelectedLocalCandidates,
    handleInstallSelectedCandidates,
  }
}
