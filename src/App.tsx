import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type SetStateAction,
} from 'react'
import './App.css'
import './figma.css'
import { useTranslation } from 'react-i18next'
import { Toaster, toast } from 'sonner'
import {
  isTauri,
  invokeTauri,
  useFormatErrorMessage,
  useShowActionErrors,
  useFormatRelative,
  getGithubInfo,
  getSkillSourceLabel,
} from './hooks/useTauriInvoke'
import { useAppSettings } from './hooks/useAppSettings'
import ExplorePage from './components/skills/ExplorePage'
import FilterBar from './components/skills/FilterBar'
import SkillDetailView from './components/skills/SkillDetailView'
import Header from './components/skills/Header'
import LoadingOverlay from './components/skills/LoadingOverlay'
import SkillsList from './components/skills/SkillsList'
import TagsPage from './components/skills/TagsPage'
import AddSkillModal from './components/skills/modals/AddSkillModal'
import BulkDeleteModal from './components/skills/modals/BulkDeleteModal'
import BulkSyncModal from './components/skills/modals/BulkSyncModal'
import BulkTagsModal from './components/skills/modals/BulkTagsModal'
import DeleteModal from './components/skills/modals/DeleteModal'
import DiscoveryScanModal from './components/skills/modals/DiscoveryScanModal'
import EditSkillTagsModal from './components/skills/modals/EditSkillTagsModal'
import GitPickModal from './components/skills/modals/GitPickModal'
import LocalPickModal from './components/skills/modals/LocalPickModal'
import ImportModal from './components/skills/modals/ImportModal'
import NewToolsModal from './components/skills/modals/NewToolsModal'
import RenameTagModal from './components/skills/modals/RenameTagModal'
import ScopeSyncModal from './components/skills/modals/ScopeSyncModal'
import SharedDirModal from './components/skills/modals/SharedDirModal'
import ToggleToolConfirmModal from './components/skills/modals/ToggleToolConfirmModal'
import SettingsPage from './components/skills/SettingsPage'
import ToolsPage from './components/skills/ToolsPage'
import UpdatesPage from './components/skills/UpdatesPage'
import WindowResizeHandles from './components/WindowResizeHandles'
import { useAutoUpdate } from './hooks/useAutoUpdate'
import { useDiscoveryScan } from './hooks/useDiscoveryScan'
import { useBulkOperations } from './hooks/useBulkOperations'
import { useSkillFilters } from './hooks/useSkillFilters'
import { useTagOperations } from './hooks/useTagOperations'
import { useSkillImport } from './hooks/useSkillImport'
import { useToolSync } from './hooks/useToolSync'
import { useSkillScope, type SkillScopeState } from './hooks/useSkillScope'
import { useSkillLifecycle } from './hooks/useSkillLifecycle'
import { useSystemSettings } from './hooks/useSystemSettings'
import { useImportPlan } from './hooks/useImportPlan'
import { useInstallScopeTargets } from './hooks/useInstallScopeTargets'
import {
  getSkillSyncState,
  getToolSyncState,
  isActiveSkillTarget,
} from './components/skills/skillSyncStatus'
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
} from './components/skills/installScope'
import type {
  AutoUpdateConfigDto,
  DiscoveryScanSettingsDto,
  FeaturedSkillDto,
  GitSkillCandidate,
  GithubProxyConfigDto,
  InstallResultDto,
  LocalSkillCandidate,
  ManagedSkill,
  OnboardingPlan,
  OnlineSkillDto,
  TagWithCountDto,
  ToolConfigDto,
  ToolOption,
  ToolStatusDto,
  UpdateResultDto,
} from './components/skills/types'


type ActiveView = 'myskills' | 'explore' | 'detail' | 'settings' | 'manage'
type ManagementTab = 'tags' | 'tools' | 'updates'

function App() {
  const { t } = useTranslation()
  const {
    language,
    toggleLanguage,
    themePreference,
    setThemePreference,
    defaultSyncMode,
    setDefaultSyncMode,
    handleDefaultSyncModeChange,
    appVersion,
    skillViewMode,
    setSkillViewMode,
    sidebarCollapsed,
    setSidebarCollapsed,
  } = useAppSettings()
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [actionMessage, setActionMessage] = useState<string | null>(null)
  const [successToastMessage, setSuccessToastMessage] = useState<string | null>(
    null,
  )
  const [managedSkills, setManagedSkills] = useState<ManagedSkill[]>([])
  const [loadingStartAt, setLoadingStartAt] = useState<number | null>(null)
  const [toolStatus, setToolStatus] = useState<ToolStatusDto | null>(null)
  const [showNewToolsModal, setShowNewToolsModal] = useState(false)
  const [activeView, setActiveView] = useState<ActiveView>('myskills')
  const [managementTab, setManagementTab] = useState<ManagementTab>('tags')
  const [detailSkill, setDetailSkill] = useState<ManagedSkill | null>(null)
  const [featuredSkills, setFeaturedSkills] = useState<FeaturedSkillDto[]>([])
  const [featuredLoading, setFeaturedLoading] = useState(false)
  const [exploreFilter, setExploreFilter] = useState('')
  const [searchResults, setSearchResults] = useState<OnlineSkillDto[]>([])
  const [searchLoading, setSearchLoading] = useState(false)
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const formatErrorMessage = useFormatErrorMessage()
  const showActionErrors = useShowActionErrors()
  const isSkillNameTaken = useCallback(
    (name: string) =>
      managedSkills.some((skill) => skill.name.toLowerCase() === name.toLowerCase()),
    [managedSkills],
  )

  const formatRelative = useFormatRelative()



  const loadManagedSkills = useCallback(async () => {
    try {
      const result = await invokeTauri<ManagedSkill[]>('get_managed_skills')
      setManagedSkills(result)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }, [invokeTauri])



  useEffect(() => {
    if (isTauri) {
      loadManagedSkills()
      loadTags()
    }
  }, [isTauri, loadManagedSkills, loadTags])








  useEffect(() => {
    if (!successToastMessage) return
    toast.success(successToastMessage, { duration: 1800 })
    setSuccessToastMessage(null)
  }, [successToastMessage])

  useEffect(() => {
    if (!error) return
    const msg = formatErrorMessage(error)
    if (msg) toast.error(msg, { duration: 2600 })
    setError(null)
    setActionMessage(null)
  }, [error, formatErrorMessage])

  // 拦截全局默认浏览器右键菜单（防止出现网页级图片另存为等菜单），保留输入框文字编辑
  useEffect(() => {
    const handleContextMenu = (e: MouseEvent) => {
      try {
        const target = e.target as HTMLElement | null
        if (!target) return
        const isEditable =
          target.tagName === 'INPUT' ||
          target.tagName === 'TEXTAREA' ||
          (typeof target.closest === 'function' &&
            Boolean(target.closest('input, textarea, [contenteditable="true"]')))
        if (!isEditable) {
          e.preventDefault()
        }
      } catch {
        // 安全拦截，不抛异常
      }
    }
    window.addEventListener('contextmenu', handleContextMenu)
    return () => window.removeEventListener('contextmenu', handleContextMenu)
  }, [])

  const toolInfos = useMemo(() => toolStatus?.tools ?? [], [toolStatus])
  const enabledToolInfos = useMemo(
    () => toolInfos.filter((info) => info.enabled),
    [toolInfos],
  )

  const tools: ToolOption[] = useMemo(() => {
    return enabledToolInfos.map((info) => ({
      id: info.key,
      // Prefer i18n label if present; fallback to backend label.
      label: t(`tools.${info.key}`, { defaultValue: info.label }),
      avatar: info.avatar,
      supports_project_scope: info.supports_project_scope,
    }))
  }, [t, enabledToolInfos])

  const toolLabelById = useMemo(() => {
    const out: Record<string, string> = {}
    for (const tool of tools) out[tool.id] = tool.label
    return out
  }, [tools])

  const sharedToolIdsByToolId = useMemo(() => {
    // toolId -> all toolIds that share the same skills_dir.
    const byDir: Record<string, string[]> = {}
    for (const info of enabledToolInfos) {
      const dir = info.skills_dir
      if (!byDir[dir]) byDir[dir] = []
      byDir[dir].push(info.key)
    }
    const out: Record<string, string[]> = {}
    for (const dir of Object.keys(byDir)) {
      const ids = byDir[dir]
      if (ids.length <= 1) continue
      for (const id of ids) out[id] = ids
    }
    return out
  }, [enabledToolInfos])

  const sharedProjectToolIdsByToolId = useMemo(() => {
    const byDir: Record<string, string[]> = {}
    for (const info of enabledToolInfos) {
      const dir = info.project_skills_dir
      if (!byDir[dir]) byDir[dir] = []
      byDir[dir].push(info.key)
    }
    const out: Record<string, string[]> = {}
    for (const ids of Object.values(byDir)) {
      if (ids.length <= 1) continue
      for (const id of ids) out[id] = ids
    }
    return out
  }, [enabledToolInfos])

  const uniqueToolIdsBySkillsDir = useCallback(
    (toolIds: string[]) => {
      // Preserve UI order (tools array order), de-dupe by skills_dir.
      const wanted = new Set(toolIds)
      const seen = new Set<string>()
      const out: string[] = []
      for (const tool of enabledToolInfos) {
        if (!wanted.has(tool.key)) continue
        if (seen.has(tool.skills_dir)) continue
        seen.add(tool.skills_dir)
        out.push(tool.key)
      }
      return out
    },
    [enabledToolInfos],
  )

  const uniqueToolIdsByProjectSkillsDir = useCallback(
    (toolIds: string[]) => {
      const wanted = new Set(toolIds)
      const seen = new Set<string>()
      const out: string[] = []
      for (const tool of enabledToolInfos) {
        if (!wanted.has(tool.key)) continue
        if (seen.has(tool.project_skills_dir)) continue
        seen.add(tool.project_skills_dir)
        out.push(tool.key)
      }
      return out
    },
    [enabledToolInfos],
  )

  const installedToolIds = useMemo(
    () => toolStatus?.installed ?? [],
    [toolStatus],
  )
  const isInstalled = useCallback(
    (id: string) => installedToolIds.includes(id),
    [installedToolIds],
  )
  const installedTools = useMemo(
    () => tools.filter((tool) => installedToolIds.includes(tool.id)),
    [tools, installedToolIds],
  )
  const toolSupportsProjectScope = useCallback(
    (toolId: string) =>
      tools.find((tool) => tool.id === toolId)?.supports_project_scope ?? true,
    [tools],
  )
  const installedProjectToolIds = useMemo(
    () => installedToolIds.filter((toolId) => toolSupportsProjectScope(toolId)),
    [installedToolIds, toolSupportsProjectScope],
  )

  const {
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
  } = useInstallScopeTargets({
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
  })

  const {
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
  } = useImportPlan({
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
  })

  const {
    discoveryScanSettings,
    showDiscoveryScanModal,
    discoveryScanSaving,
    discoveryScanEnabledCount,
    discoveryScanSourceCount,
    handleOpenDiscoveryScanSettings,
    handleCloseDiscoveryScanSettings,
    handleSaveDiscoveryScanSettings,
  } = useDiscoveryScan({
    onError: setError,
    onPlanReload: reloadPlanQuietly,
  })


  const {
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
  } = useSkillImport({
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
  })

  const {
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
  } = useSkillScope({
    managedSkills,
    installedToolIds,
    installedProjectToolIds,
    loading,
    setLoading,
    setLoadingStartAt,
    setError,
    loadManagedSkills,
  })

  const {
    pendingDeleteId,
    setPendingDeleteId,
    pendingDeleteSkill,
    handleDeletePrompt,
    handleCloseDelete,
    handleDeleteManaged,
    handleToggleSkillEnabled,
    handleUpdateManaged,
    handleUpdateSkill,
  } = useSkillLifecycle({
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
  })

  const toolOptionsWithCounts = useMemo(() => {
    return installedTools.map((tool) => {
      const count = managedSkills.filter(
        (skill) => getToolSyncState(skill, tool.id, getSkillScope(skill)) === 'synced',
      ).length
      return {
        id: tool.id,
        label: tool.label,
        avatar: tool.avatar,
        count,
      }
    })
  }, [installedTools, managedSkills, getSkillScope])

  const toolCountsByToolId = useMemo(() => {
    const map: Record<string, number> = {}
    for (const opt of toolOptionsWithCounts) {
      map[opt.id] = opt.count
    }
    return map
  }, [toolOptionsWithCounts])

  const {
    searchQuery,
    setSearchQuery,
    sortBy,
    setSortBy,
    scopeFilter,
    setScopeFilter,
    toolFilter,
    setToolFilter,
    selectedTagIds,
    setSelectedTagIds,
    includeUntagged,
    setIncludeUntagged,
    visibleSkills,
    untaggedCount,
    handleToggleTagFilter,
    handleToggleUntaggedFilter,
    handleClearTagFilters,
    filterByTagId,
    filterByUntagged,
  } = useSkillFilters({
    managedSkills,
    getSkillScope,
  })

  const {
    tags,
    setTags,
    loadTags,
    tagEditorSkill,
    pendingRenameTag,
    pendingDeleteTag,
    handleCreateTag,
    handleRenameTag,
    handleCloseRenameTag,
    handleDeleteTag,
    handleCloseDeleteTag,
    handleConfirmDeleteTag,
    handleOpenEditTags,
    handleCloseEditTags,
    handleSaveSkillTags,
  } = useTagOperations({
    loading,
    setLoading,
    setLoadingStartAt,
    setActionMessage,
    setSuccessToastMessage,
    setError,
    loadManagedSkills,
    onTagDeleted: (deletedTagId) => {
      setSelectedTagIds((current) => current.filter((id) => id !== deletedTagId))
    },
    onTagRenamed: (oldTagId, newTagId) => {
      setSelectedTagIds((current) =>
        current.includes(oldTagId)
          ? current.map((id) => (id === oldTagId ? newTagId : id))
          : current,
      )
    },
  })
  const {
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
    showBulkDeleteModal,
    showBulkTagsModal,
    bulkSyncToolIds,
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
  } = useBulkOperations({
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
  })

  const {
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
  } = useToolSync({
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
  })

  const {
    storagePath,
    gitCacheTtlSecs,
    githubProxyConfig,
    toolConfig,
    handleGitCacheTtlSecsChange,
    handleGithubProxyConfigChange,
    handleToolConfigChange,
    handleClearGitCacheNow,
  } = useSystemSettings({
    setError,
    setSuccessToastMessage,
    setToolStatus,
    setSyncTargets,
  })
  const {
    autoUpdateConfig,
    autoUpdateTriggering,
    handleAutoUpdateConfigChange,
    handleTriggerAutoUpdateTaskNow,
  } = useAutoUpdate({
    activeView,
    managementTab,
    onSkillsReload: loadManagedSkills,
    onSuccessToast: setSuccessToastMessage,
    onError: setError,
  })

  const handlePickLocalPath = useCallback(async () => {
    try {
      if (!isTauri) {
        throw new Error(t('errors.notTauri'))
      }
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({
        directory: true,
        multiple: false,
        title: t('selectLocalFolder'),
      })
      if (!selected || Array.isArray(selected)) return
      setLocalPath(selected)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }, [isTauri, t])

  const newlyInstalledToolsText = useMemo(() => {
    if (!toolStatus || toolStatus.newly_installed.length === 0) return ''
    return toolStatus.newly_installed
      .map((id) => tools.find((t) => t.id === id)?.label ?? id)
      .join('、')
  }, [toolStatus, tools])

  const handleOpenSettings = useCallback(() => {
    setShowAddModal(false)
    setActiveView('settings')
  }, [])



  const loadFeaturedSkills = useCallback(async () => {
    if (featuredSkills.length > 0) return
    setFeaturedLoading(true)
    try {
      const result = await invokeTauri<FeaturedSkillDto[]>('get_featured_skills')
      setFeaturedSkills(result)
    } catch {
      // silent — explore tab will show empty state
    } finally {
      setFeaturedLoading(false)
    }
  }, [featuredSkills.length, invokeTauri])

  const handleViewChange = useCallback(
    (view: 'myskills' | 'explore' | 'manage') => {
      setShowAddModal(false)
      setActiveView(view)
      if (view !== 'myskills') {
        resetBulkState()
      }
      if (view === 'explore') {
        loadFeaturedSkills()
      }
      if (view === 'manage') {
        setManagementTab('tags')
      }
      if (view === 'myskills') {
        setDetailSkill(null)
      }
    },
    [loadFeaturedSkills, resetBulkState],
  )

  const handleOpenDetail = useCallback((skill: ManagedSkill) => {
    resetBulkState()
    setDetailSkill(skill)
    setActiveView('detail')
  }, [resetBulkState])

  const handleBackToList = useCallback(() => {
    setDetailSkill(null)
    setActiveView('myskills')
  }, [])

  const handleExploreFilterChange = useCallback(
    (value: string) => {
      setExploreFilter(value)
      if (searchTimerRef.current) {
        clearTimeout(searchTimerRef.current)
        searchTimerRef.current = null
      }
      if (value.trim().length < 2) {
        setSearchResults([])
        setSearchLoading(false)
        return
      }
      setSearchLoading(true)
      searchTimerRef.current = setTimeout(async () => {
        try {
          const results = await invokeTauri<OnlineSkillDto[]>(
            'search_skills_online',
            { query: value.trim(), limit: 20 },
          )
          setSearchResults(results)
        } catch {
          toast.error(t('searchError'))
          setSearchResults([])
        } finally {
          setSearchLoading(false)
        }
      }, 500)
    },
    [invokeTauri, t],
  )


  const handleOpenAdd = useCallback((tab: 'git' | 'local' = 'git') => {
    resetInstallScope()
    setAddModalTab(tab)
    setShowAddModal(true)
    setAddModalTagIds([])
  }, [resetInstallScope])

  const handleCancelLoading = useCallback(() => {
    void invokeTauri('cancel_current_operation').catch(() => {})
    setLoading(false)
    setLoadingStartAt(null)
    setActionMessage(null)
  }, [invokeTauri])



  const handleCloseSettings = useCallback(() => {
    setActiveView('myskills')
  }, [])

  const handleThemeChange = useCallback(
    (nextTheme: 'system' | 'light' | 'dark') => {
      setThemePreference(nextTheme)
    },
    [],
  )

  const handleCloseNewTools = useCallback(() => {
    if (!loading) setShowNewToolsModal(false)
  }, [loading])

  const handleSortChange = useCallback((value: 'updated' | 'name') => {
    setSortBy(value)
  }, [])

  const handleSearchChange = useCallback((value: string) => {
    setSearchQuery(value)
  }, [])

  const handleScopeFilterChange = useCallback(
    (value: 'all' | 'global' | 'project') => {
      setScopeFilter(value)
    },
    [],
  )

  const handleToggleAddModalTag = useCallback((tagId: number) => {
    setAddModalTagIds((current) =>
      current.includes(tagId)
        ? current.filter((id) => id !== tagId)
        : [...current, tagId],
    )
  }, [])

  const handleReviewUntagged = useCallback(() => {
    filterByUntagged()
    setActiveView('myskills')
  }, [filterByUntagged])

  const handleViewTag = useCallback((tagId: number) => {
    filterByTagId(tagId)
    setActiveView('myskills')
  }, [filterByTagId])

  const handleOpenTagsPage = useCallback(() => {
    setManagementTab('tags')
    setActiveView('manage')
  }, [])









  useEffect(() => {
    const load = async () => {
      if (!isTauri) return
      try {
        const status = await invokeTauri<ToolStatusDto>('get_tool_status')
        setToolStatus(status)

        // Default-select installed tools for sync targets if user hasn't toggled yet.
        setSyncTargets((prev) => {
          if (Object.keys(prev).length > 0) return prev
          const next: Record<string, boolean> = {}
          for (const t of status.tools) {
            next[t.key] = status.installed.includes(t.key)
          }
          return next
        })

        if (status.newly_installed.length > 0) {
          setShowNewToolsModal(true)
        }
      } catch (err) {
        // Non-fatal; app can still work without detection.
        console.warn(err)
      }
    }
    void load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isTauri])



  const [exploreInstallTrigger, setExploreInstallTrigger] = useState(0)
  const exploreInstallUrlRef = useRef<string | null>(null)

  const handleExploreInstall = useCallback(
    (sourceUrl: string, skillName?: string) => {
      resetInstallScope()
      setGitUrl(sourceUrl)
      if (skillName) setAutoSelectSkillName(skillName)
      if (toolStatus) {
        const targets: Record<string, boolean> = {}
        for (const id of toolStatus.installed) {
          targets[id] = true
        }
        setSyncTargets(targets)
      }
      exploreInstallUrlRef.current = sourceUrl
      setExploreInstallTrigger((n) => n + 1)
    },
    [resetInstallScope, toolStatus],
  )

  useEffect(() => {
    if (exploreInstallTrigger > 0 && exploreInstallUrlRef.current && !loading) {
      exploreInstallUrlRef.current = null
      void handleCreateGit()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [exploreInstallTrigger])


  const handleSyncAllManagedToTools = useCallback(
    async (toolIds: string[]) => {
      if (managedSkills.length === 0) return
      const installedIds = uniqueToolIdsBySkillsDir(
        toolIds.filter((id) => isInstalled(id)),
      )
      if (installedIds.length === 0) return

      setLoading(true)
      setLoadingStartAt(Date.now())
      setError(null)
      try {
        const collectedErrors: { title: string; message: string }[] = []
        for (let si = 0; si < managedSkills.length; si++) {
          const skill = managedSkills[si]
          const skillScope = getSkillScope(skill)
          const projects = getSkillProjects(skill)
          for (let ti = 0; ti < installedIds.length; ti++) {
            const toolId = installedIds[ti]
            const toolLabel = tools.find((t) => t.id === toolId)?.label ?? toolId
            if (skillScope === 'project') {
              if (!toolSupportsProjectScope(toolId)) continue
              if (projects.length === 0) continue
            }
            setActionMessage(
              t('actions.syncStep', {
                index: si + 1,
                total: managedSkills.length,
                name: skill.name,
                tool: toolLabel,
              }),
            )
            try {
              if (skillScope === 'project') {
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
              const raw = err instanceof Error ? err.message : String(err)
              collectedErrors.push({
                title: t('errors.syncFailedTitle', {
                  name: skill.name,
                  tool: toolLabel,
                }),
                message: raw.startsWith('TOOL_NOT_INSTALLED|')
                  ? t('errors.toolNotInstalled')
                  : raw.startsWith('TOOL_NOT_WRITABLE|')
                    ? t('errors.toolNotWritable', {
                        tool: raw.split('|')[1] ?? toolLabel,
                        path: raw.split('|')[2] ?? '',
                      })
                    : raw,
              })
            }
          }
        }
        await loadManagedSkills()
        if (collectedErrors.length > 0) {
          showActionErrors(collectedErrors)
        } else {
          setSuccessToastMessage(t('status.syncCompleted'))
        }
        setActionMessage(null)
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
      }
    },
    [
      invokeTauri,
      getSkillProjects,
      getSkillScope,
      isInstalled,
      loadManagedSkills,
      managedSkills,
      showActionErrors,
      t,
      tools,
      toolSupportsProjectScope,
      uniqueToolIdsBySkillsDir,
    ],
  )

  const handleSyncAllNewTools = useCallback(() => {
    if (!toolStatus) return
    setSyncTargets((prev) => {
      const next = { ...prev }
      for (const id of toolStatus.newly_installed) {
        const shared = sharedToolIdsByToolId[id] ?? [id]
        for (const sid of shared) next[sid] = true
      }
      return next
    })
    setShowNewToolsModal(false)
    void handleSyncAllManagedToTools(toolStatus.newly_installed)
  }, [handleSyncAllManagedToTools, sharedToolIdsByToolId, toolStatus])









  const globalSkillCount = managedSkills.filter(
    (skill) => getSkillScope(skill) === 'global',
  ).length
  const projectSkillCount = managedSkills.length - globalSkillCount
  const enabledSkillCount = managedSkills.filter((skill) => skill.enabled !== false).length
  const syncIssueCount = managedSkills.filter((skill) => {
    const state = getSkillSyncState(skill)
    return state === 'source-error' || state === 'partial' || state === 'failed'
  }).length
  const unsyncedSkillCount = managedSkills.filter(
    (skill) => getSkillSyncState(skill) === 'idle',
  ).length
  const dashboardSyncStatus =
    syncIssueCount > 0
      ? { className: 'error', label: t('stats.needsAttention', { count: syncIssueCount }) }
      : unsyncedSkillCount > 0
        ? { className: 'idle', label: t('stats.notSyncedCount', { count: unsyncedSkillCount }) }
        : enabledSkillCount === managedSkills.length
          ? { className: 'healthy', label: t('stats.allNormal') }
          : { className: 'idle', label: t('stats.enabledCount', { count: enabledSkillCount }) }
  const pendingUpdateCount = autoUpdateConfig?.last_failed ?? 0

  const handleManagementTabChange = (tab: ManagementTab) => {
    setShowAddModal(false)
    setManagementTab(tab)
    setActiveView('manage')
  }

  return (
    <div className={`skills-app${isTauri ? ' is-tauri' : ''}${sidebarCollapsed ? ' sidebar-collapsed' : ''}`}>
      <Toaster
        position="top-right"
        richColors
        toastOptions={{ duration: 1800 }}
      />
      <LoadingOverlay
        loading={loading}
        actionMessage={actionMessage}
        loadingStartAt={loadingStartAt}
        onCancel={handleCancelLoading}
        t={t}
      />

      <Header
        activeView={activeView}
        managementTab={managementTab}
        skillCount={managedSkills.length}
        tagCount={tags.length}
        toolCount={toolStatus?.tools.length ?? 0}
        updateCount={pendingUpdateCount}
        appVersion={appVersion}
        collapsed={sidebarCollapsed}
        onToggleCollapsed={() => setSidebarCollapsed((collapsed) => !collapsed)}
        onOpenSettings={handleOpenSettings}
        onViewChange={handleViewChange}
        onManagementTabChange={handleManagementTabChange}
        t={t}
      />
      <WindowResizeHandles enabled={isTauri} />

      <main className="skills-main">
        {activeView === 'detail' && detailSkill ? (
          <SkillDetailView
            skill={detailSkill}
            onBack={handleBackToList}
            invokeTauri={invokeTauri}
            formatRelative={formatRelative}
            tools={installedTools}
            scope={getSkillScope(detailSkill)}
            projects={getSkillProjects(detailSkill)}
            t={t}
            onRefresh={loadManagedSkills}
          />
        ) : activeView === 'myskills' ? (
          <div className="dashboard-stack">
            <section className="dashboard-stats" aria-label={t('navMySkills')}>
              <article>
                <span>{t('stats.managed')}</span>
                <strong>{managedSkills.length}</strong>
              </article>
              <article>
                <span>{t('stats.global')}</span>
                <strong>{globalSkillCount}</strong>
              </article>
              <article>
                <span>{t('stats.project')}</span>
                <strong>{projectSkillCount}</strong>
              </article>
              <article className={`dashboard-status-card${syncIssueCount > 0 ? ' actionable' : ''}`}>
                <button
                  className="dashboard-status-button"
                  type="button"
                  disabled={syncIssueCount === 0}
                  onClick={() => handleManagementTabChange('updates')}
                  aria-label={
                    syncIssueCount > 0
                      ? `${dashboardSyncStatus.label}，${t('stats.viewIssues')}`
                      : dashboardSyncStatus.label
                  }
                  title={syncIssueCount > 0 ? t('stats.viewIssues') : undefined}
                >
                  <span>{t('stats.syncStatus')}</span>
                  <strong className={`status-summary ${dashboardSyncStatus.className}`}>
                    <i />{dashboardSyncStatus.label}
                  </strong>
                  {syncIssueCount > 0 ? (
                    <span className="dashboard-status-action">
                      {t('stats.viewIssues')} <span aria-hidden="true">→</span>
                    </span>
                  ) : null}
                </button>
              </article>
            </section>
            <FilterBar
              sortBy={sortBy}
              searchQuery={searchQuery}
              scopeFilter={scopeFilter}
              toolFilter={toolFilter}
              toolOptions={toolOptionsWithCounts}
              tags={tags}
              selectedTagIds={selectedTagIds}
              includeUntagged={includeUntagged}
              untaggedCount={untaggedCount}
              totalCount={visibleSkills.length}
              bulkMode={bulkMode}
              bulkSelectedCount={bulkSelectedIds.length}
              viewMode={skillViewMode}
              onSortChange={handleSortChange}
              onSearchChange={handleSearchChange}
              onScopeFilterChange={handleScopeFilterChange}
              onToolFilterChange={setToolFilter}
              onToggleTag={handleToggleTagFilter}
              onToggleUntagged={handleToggleUntaggedFilter}
              onClearTags={handleClearTagFilters}
              onManageTags={handleOpenTagsPage}
              onToggleBulkMode={handleToggleBulkMode}
              onViewModeChange={setSkillViewMode}
              t={t}
            />
            <SkillsList
              plan={plan}
              visibleSkills={visibleSkills}
              installedTools={installedTools}
              toolCounts={toolCountsByToolId}
              loading={loading}
              bulkMode={bulkMode}
              selectedSkillIds={bulkSelectedIds}
              viewMode={skillViewMode}
              getGithubInfo={getGithubInfo}
              getSkillSourceLabel={getSkillSourceLabel}
              formatRelative={formatRelative}
              onReviewImport={handleReviewImport}
              onOpenScanSettings={handleOpenDiscoveryScanSettings}
              onUpdateSkill={handleUpdateSkill}
              onDeleteSkill={handleDeletePrompt}
              onToggleSkillEnabled={handleToggleSkillEnabled}
              onToggleTool={handleToggleToolForSkill}
              onSyncToolWithMode={handleSyncToolWithMode}
              onUnsyncTool={handleUnsyncToolForSkill}
              onOpenScope={handleOpenScope}
              onOpenDetail={handleOpenDetail}
              onEditTags={handleOpenEditTags}
              onToggleBulkSelection={handleToggleBulkSelection}
              getSkillScope={getSkillScope}
              getSkillProjects={getSkillProjects}
              t={t}
            />
            {bulkMode ? (
              <div className="bulk-action-bar">
                <div className="bulk-action-copy">
                  <strong>{t('bulk.selected', { count: bulkSelectedIds.length })}</strong>
                  <span>{t('bulk.helper')}</span>
                </div>
                <div className="bulk-action-buttons">
                  <button
                    className="btn btn-secondary"
                    type="button"
                    onClick={handleSelectVisibleSkills}
                    disabled={loading || visibleSkills.length === 0}
                  >
                    {allVisibleBulkSelected
                      ? t('bulk.unselectVisible')
                      : t('bulk.selectVisible')}
                  </button>
                  <button
                    className="btn btn-secondary"
                    type="button"
                    onClick={handleOpenBulkTags}
                    disabled={loading || bulkSelectedIds.length === 0}
                  >
                    {t('bulk.tags')}
                  </button>
                  <button
                    className="btn btn-secondary"
                    type="button"
                    onClick={handleOpenBulkSync}
                    disabled={
                      loading ||
                      bulkSelectedIds.length === 0 ||
                      bulkHasDisabledSelected
                    }
                  >
                    {t('bulk.sync')}
                  </button>
                  <button
                    className="btn btn-secondary"
                    type="button"
                    onClick={() => void handleBulkUpdate()}
                    disabled={loading || bulkUpdateableSkills.length === 0}
                    title={
                      bulkSelectedIds.length > 0 && bulkUpdateableSkills.length === 0
                        ? t('bulk.noUpdateSources')
                        : undefined
                    }
                  >
                    {t('bulk.update')}
                  </button>
                  <button
                    className="btn btn-secondary"
                    type="button"
                    onClick={() => void handleToggleBulkEnabled()}
                    disabled={loading || bulkSelectedIds.length === 0}
                  >
                    {bulkShouldEnable ? t('bulk.enable') : t('bulk.disable')}
                  </button>
                  <button
                    className="btn btn-danger"
                    type="button"
                    onClick={handleOpenBulkDelete}
                    disabled={loading || bulkSelectedIds.length === 0}
                  >
                    {t('bulk.delete')}
                  </button>
                </div>
              </div>
            ) : null}
          </div>
        ) : activeView === 'manage' ? (
          <div className="management-page">
            <div className="management-header">
              <div className="management-heading">
                <h1>{t('manageCenterTitle')}</h1>
                <p>{t('manageCenterHelp')}</p>
              </div>
              <div className="management-tab-list" role="tablist" aria-label={t('manageCenterTitle')}>
                <button
                  className={`management-tab${managementTab === 'tags' ? ' active' : ''}`}
                  type="button"
                  role="tab"
                  aria-selected={managementTab === 'tags'}
                  onClick={() => setManagementTab('tags')}
                >
                  {t('manageTabs.tags')}
                </button>
                <button
                  className={`management-tab${managementTab === 'tools' ? ' active' : ''}`}
                  type="button"
                  role="tab"
                  aria-selected={managementTab === 'tools'}
                  onClick={() => setManagementTab('tools')}
                >
                  {t('manageTabs.tools')}
                </button>
                <button
                  className={`management-tab${managementTab === 'updates' ? ' active' : ''}`}
                  type="button"
                  role="tab"
                  aria-selected={managementTab === 'updates'}
                  onClick={() => setManagementTab('updates')}
                >
                  {t('manageTabs.updates')}
                </button>
              </div>
            </div>
            <div className="management-panel">
              {managementTab === 'tags' ? (
                <TagsPage
                  embedded
                  tags={tags}
                  untaggedCount={untaggedCount}
                  loading={loading}
                  formatRelative={formatRelative}
                  onReviewUntagged={handleReviewUntagged}
                  onViewTag={handleViewTag}
                  onCreateTag={handleCreateTag}
                  onRenameTag={setPendingRenameTag}
                  onDeleteTag={handleDeleteTag}
                  t={t}
                />
              ) : managementTab === 'tools' ? (
                <ToolsPage
                  embedded
                  toolStatus={toolStatus}
                  toolConfig={toolConfig}
                  onToolConfigChange={handleToolConfigChange}
                  t={t}
                />
              ) : (
                <UpdatesPage
                  autoUpdateConfig={autoUpdateConfig}
                  onAutoUpdateConfigChange={handleAutoUpdateConfigChange}
                  onRunAutoUpdateNow={handleTriggerAutoUpdateTaskNow}
                  autoUpdateTriggering={autoUpdateTriggering}
                  t={t}
                />
              )}
            </div>
          </div>
        ) : activeView === 'settings' ? (
          <SettingsPage
            isTauri={isTauri}
            language={language}
            storagePath={storagePath}
            gitCacheTtlSecs={gitCacheTtlSecs}
            themePreference={themePreference}
            defaultSyncMode={defaultSyncMode}
            onToggleLanguage={toggleLanguage}
            onThemeChange={handleThemeChange}
            onDefaultSyncModeChange={handleDefaultSyncModeChange}
            onGitCacheTtlSecsChange={handleGitCacheTtlSecsChange}
            onClearGitCacheNow={handleClearGitCacheNow}
            githubProxyConfig={githubProxyConfig}
            onGithubProxyConfigChange={handleGithubProxyConfigChange}
            discoveryScanEnabledCount={discoveryScanEnabledCount}
            discoveryScanSourceCount={discoveryScanSourceCount}
            onOpenDiscoveryScanSettings={handleOpenDiscoveryScanSettings}
            onBack={handleCloseSettings}
            t={t}
          />
        ) : (
          <ExplorePage
            featuredSkills={featuredSkills}
            featuredLoading={featuredLoading}
            exploreFilter={exploreFilter}
            searchResults={searchResults}
            searchLoading={searchLoading}
            managedSkills={managedSkills}
            loading={loading}
            onExploreFilterChange={handleExploreFilterChange}
            onInstallSkill={handleExploreInstall}
            onOpenManualAdd={handleOpenAdd}
            t={t}
          />
        )}
      </main>

      <AddSkillModal
        open={showAddModal}
        loading={loading}
        canClose={!loading}
        addModalTab={addModalTab}
        localPath={localPath}
        gitUrl={gitUrl}
        tags={tags}
        selectedTagIds={addModalTagIds}
        syncTargets={syncTargets}
        installedTools={installedTools}
        toolStatus={toolStatus}
        installScope={installScope}
        installProjects={installProjects}
        recentProjects={recentProjects}
        onRequestClose={handleCloseAdd}
        onTabChange={setAddModalTab}
        onLocalPathChange={setLocalPath}
        onPickLocalPath={handlePickLocalPath}
        onGitUrlChange={setGitUrl}
        onToggleTag={handleToggleAddModalTag}
        onSyncTargetChange={handleSyncTargetChange}
        onInstallScopeChange={handleInstallScopeChange}
        onInstallProjectsChange={handleInstallProjectsChange}
        onPickProject={handlePickProject}
        onSubmit={addModalTab === 'local' ? handleCreateLocal : handleCreateGit}
        t={t}
      />

      <EditSkillTagsModal
        key={
          tagEditorSkill
            ? `${tagEditorSkill.id}-${tagEditorSkill.tags.map((tag) => tag.id).join('-')}`
            : 'edit-tags'
        }
        open={Boolean(tagEditorSkill)}
        loading={loading}
        skill={
          tagEditorSkill
            ? managedSkills.find((skill) => skill.id === tagEditorSkill.id) ?? tagEditorSkill
            : null
        }
        tags={tags}
        onRequestClose={handleCloseEditTags}
        onSave={handleSaveSkillTags}
        t={t}
      />

      <BulkSyncModal
        open={showBulkSyncModal}
        loading={loading}
        selectedCount={bulkSelectedIds.length}
        installedTools={installedTools}
        selectedToolIds={bulkSyncToolIds}
        onToggleTool={handleToggleBulkSyncTool}
        onRequestClose={handleCloseBulkSync}
        onConfirm={handleConfirmBulkSync}
        t={t}
      />

      <BulkTagsModal
        open={showBulkTagsModal}
        loading={loading}
        selectedSkills={bulkSelectedSkills}
        tags={tags}
        onRequestClose={handleCloseBulkTags}
        onConfirm={handleConfirmBulkTags}
        t={t}
      />

      <BulkDeleteModal
        open={showBulkDeleteModal}
        loading={loading}
        skillNames={bulkSelectedNames}
        onRequestClose={handleCloseBulkDelete}
        onConfirm={handleConfirmBulkDelete}
        t={t}
      />

      {showImportModal && plan ? (
        <ImportModal
          open={showImportModal}
          loading={loading}
          plan={plan}
          selected={selected}
          variantChoice={variantChoice}
          defaultSyncMode={defaultSyncMode}
          onRequestClose={handleCloseImport}
          onToggleGroup={handleToggleGroup}
          onSelectVariant={handleSelectVariant}
          onImport={handleImport}
          t={t}
        />
      ) : null}

      <DiscoveryScanModal
        open={showDiscoveryScanModal}
        saving={discoveryScanSaving}
        settings={discoveryScanSettings}
        onRequestClose={handleCloseDiscoveryScanSettings}
        onSave={handleSaveDiscoveryScanSettings}
        t={t}
      />

      <ToggleToolConfirmModal
        open={Boolean(pendingToolToggle)}
        loading={loading}
        skillName={pendingToolToggle?.skill.name ?? ''}
        toolLabel={
          pendingToolToggle
            ? (toolLabelById[pendingToolToggle.toolId] ?? pendingToolToggle.toolId)
            : ''
        }
        action={pendingToolToggle?.action ?? 'sync'}
        onRequestClose={() => setPendingToolToggle(null)}
        onConfirm={handleConfirmToolToggle}
        t={t}
      />

      <SharedDirModal
        open={Boolean(pendingSharedToggle)}
        loading={loading}
        toolLabel={pendingSharedLabels?.toolLabel ?? ''}
        otherLabels={pendingSharedLabels?.otherLabels ?? ''}
        onRequestClose={handleSharedCancel}
        onConfirm={handleSharedConfirm}
        t={t}
      />

      <SharedDirModal
        open={Boolean(pendingSyncTargetChange)}
        loading={loading}
        toolLabel={pendingSyncTargetLabels?.toolLabel ?? ''}
        otherLabels={pendingSyncTargetLabels?.otherLabels ?? ''}
        onRequestClose={handleSyncTargetChangeCancel}
        onConfirm={handleSyncTargetChangeConfirm}
        t={t}
      />

      <RenameTagModal
        key={pendingRenameTag?.id ?? 'rename-tag-modal'}
        open={Boolean(pendingRenameTag)}
        loading={loading}
        tag={pendingRenameTag}
        onRequestClose={handleCloseRenameTag}
        onSave={(tagId, name) => void handleRenameTag(tagId, name)}
        t={t}
      />

      <ScopeSyncModal
        key={
          currentScopeModalSkill
            ? `${currentScopeModalSkill.id}-${getSkillScope(currentScopeModalSkill)}`
            : 'scope-modal'
        }
        open={Boolean(currentScopeModalSkill)}
        loading={loading}
        skill={currentScopeModalSkill}
        scope={
          currentScopeModalSkill ? getSkillScope(currentScopeModalSkill) : 'global'
        }
        projects={
          currentScopeModalSkill ? getSkillProjects(currentScopeModalSkill) : []
        }
        recentProjects={recentProjects}
        onRequestClose={handleCloseScope}
        onScopeChange={handleScopeChange}
        onPickProject={handlePickProject}
        t={t}
      />

      <NewToolsModal
        open={Boolean(showNewToolsModal && newlyInstalledToolsText)}
        loading={loading}
        toolsLabelText={newlyInstalledToolsText}
        onLater={handleCloseNewTools}
        onSyncAll={handleSyncAllNewTools}
        t={t}
      />

      <DeleteModal
        open={Boolean(pendingDeleteId)}
        loading={loading}
        skillName={pendingDeleteSkill?.name ?? null}
        onRequestClose={handleCloseDelete}
        onConfirm={() => {
          if (pendingDeleteSkill) void handleDeleteManaged(pendingDeleteSkill)
        }}
        t={t}
      />

      {pendingDeleteTag ? (
        <div className="modal-backdrop" onClick={loading ? undefined : handleCloseDeleteTag}>
          <div
            className="modal modal-delete tag-delete-modal"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="modal-header">
              <div className="modal-title">{t('deleteTagTitle')}</div>
              <button
                className="modal-close"
                type="button"
                onClick={handleCloseDeleteTag}
                disabled={loading}
              >
                ×
              </button>
            </div>
            <div className="modal-body tag-delete-body">
              {t('deleteTagConfirm', {
                name: pendingDeleteTag.name,
                count: pendingDeleteTag.skill_count,
              })}
            </div>
            <div className="modal-footer">
              <button
                className="btn btn-secondary"
                type="button"
                onClick={handleCloseDeleteTag}
                disabled={loading}
              >
                {t('cancel')}
              </button>
              <button
                className="btn btn-danger"
                type="button"
                onClick={() => void handleConfirmDeleteTag()}
                disabled={loading}
              >
                {t('deleteAction')}
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {showLocalPickModal ? (
        <LocalPickModal
          open={showLocalPickModal}
          loading={loading}
          localCandidates={localCandidates}
          localCandidateSelected={localCandidateSelected}
          onRequestClose={handleCloseLocalPick}
          onCancel={handleCancelLocalPick}
          onToggleCandidate={handleToggleLocalCandidate}
          onInstall={handleInstallSelectedLocalCandidates}
          t={t}
        />
      ) : null}

      {showGitPickModal ? (
        <GitPickModal
          open={showGitPickModal}
          loading={loading}
          gitCandidates={gitCandidates}
          gitCandidateSelected={gitCandidateSelected}
          onRequestClose={handleCloseGitPick}
          onCancel={handleCancelGitPick}
          onToggleCandidate={handleToggleGitCandidate}
          onInstall={handleInstallSelectedCandidates}
          t={t}
        />
      ) : null}

      </div>
  )
}

export default App
