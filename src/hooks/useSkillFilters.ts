import { useCallback, useMemo, useState } from 'react'
import { getToolSyncState } from '../components/skills/skillSyncStatus'
import type { ManagedSkill } from '../components/skills/types'

export type SortByOption = 'updated' | 'name'
export type ScopeFilterOption = 'all' | 'global' | 'project'

export interface UseSkillFiltersOptions {
  managedSkills: ManagedSkill[]
  getSkillScope: (skill: ManagedSkill) => 'global' | 'project'
}

export interface UseSkillFiltersReturn {
  searchQuery: string
  setSearchQuery: React.Dispatch<React.SetStateAction<string>>
  sortBy: SortByOption
  setSortBy: React.Dispatch<React.SetStateAction<SortByOption>>
  scopeFilter: ScopeFilterOption
  setScopeFilter: React.Dispatch<React.SetStateAction<ScopeFilterOption>>
  toolFilter: string
  setToolFilter: React.Dispatch<React.SetStateAction<string>>
  selectedTagIds: number[]
  setSelectedTagIds: React.Dispatch<React.SetStateAction<number[]>>
  includeUntagged: boolean
  setIncludeUntagged: React.Dispatch<React.SetStateAction<boolean>>
  visibleSkills: ManagedSkill[]
  untaggedCount: number
  handleToggleTagFilter: (tagId: number) => void
  handleToggleUntaggedFilter: () => void
  handleClearTagFilters: () => void
  filterByTagId: (tagId: number) => void
  filterByUntagged: () => void
  resetFilters: () => void
}

export function useSkillFilters({
  managedSkills,
  getSkillScope,
}: UseSkillFiltersOptions): UseSkillFiltersReturn {
  const [searchQuery, setSearchQuery] = useState('')
  const [sortBy, setSortBy] = useState<SortByOption>('updated')
  const [scopeFilter, setScopeFilter] = useState<ScopeFilterOption>('all')
  const [toolFilter, setToolFilter] = useState<string>('all')
  const [selectedTagIds, setSelectedTagIds] = useState<number[]>([])
  const [includeUntagged, setIncludeUntagged] = useState(false)

  const visibleSkills = useMemo(() => {
    const query = searchQuery.trim().toLowerCase()
    const selectedTagSet = new Set(selectedTagIds)
    const hasTagFilter = selectedTagIds.length > 0 || includeUntagged
    const filtered = managedSkills.filter((skill) => {
      if (scopeFilter !== 'all' && getSkillScope(skill) !== scopeFilter) return false
      if (toolFilter !== 'all') {
        const isSynced = getToolSyncState(skill, toolFilter, getSkillScope(skill)) === 'synced'
        if (!isSynced) return false
      }
      if (hasTagFilter) {
        const matchesSelectedTag = skill.tags.some((tag) => selectedTagSet.has(tag.id))
        const matchesUntagged = includeUntagged && skill.tags.length === 0
        if (!matchesSelectedTag && !matchesUntagged) return false
      }
      if (!query) return true
      return (
        skill.name.toLowerCase().includes(query) ||
        skill.central_path.toLowerCase().includes(query) ||
        skill.source_type.toLowerCase().includes(query) ||
        skill.tags.some((tag) => tag.name.toLowerCase().includes(query))
      )
    })
    const sorted = [...filtered].sort((a, b) => {
      if (sortBy === 'name') {
        return a.name.localeCompare(b.name)
      }
      return (b.updated_at ?? 0) - (a.updated_at ?? 0)
    })
    return sorted
  }, [
    getSkillScope,
    includeUntagged,
    managedSkills,
    scopeFilter,
    toolFilter,
    searchQuery,
    selectedTagIds,
    sortBy,
  ])

  const untaggedCount = useMemo(
    () => managedSkills.filter((skill) => skill.tags.length === 0).length,
    [managedSkills],
  )

  const handleToggleTagFilter = useCallback((tagId: number) => {
    setSelectedTagIds((current) =>
      current.includes(tagId)
        ? current.filter((id) => id !== tagId)
        : [...current, tagId],
    )
  }, [])

  const handleToggleUntaggedFilter = useCallback(() => {
    setIncludeUntagged((current) => !current)
  }, [])

  const handleClearTagFilters = useCallback(() => {
    setSelectedTagIds([])
    setIncludeUntagged(false)
  }, [])

  const filterByTagId = useCallback((tagId: number) => {
    setSelectedTagIds([tagId])
    setIncludeUntagged(false)
  }, [])

  const filterByUntagged = useCallback(() => {
    setSelectedTagIds([])
    setIncludeUntagged(true)
  }, [])

  const resetFilters = useCallback(() => {
    setSearchQuery('')
    setSortBy('updated')
    setScopeFilter('all')
    setToolFilter('all')
    setSelectedTagIds([])
    setIncludeUntagged(false)
  }, [])

  return {
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
    resetFilters,
  }
}
