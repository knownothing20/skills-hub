import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { ArrowUpDown, Check, CheckSquare, ChevronDown, LayoutGrid, Layers, List, Search, Tags } from 'lucide-react'
import type { TFunction } from 'i18next'
import type { TagWithCountDto, ToolOptionWithCount } from './types'

type FilterBarProps = {
  sortBy: 'updated' | 'name'
  searchQuery: string
  scopeFilter: 'all' | 'global' | 'project'
  toolFilter: string
  toolOptions: ToolOptionWithCount[]
  tags: TagWithCountDto[]
  selectedTagIds: number[]
  includeUntagged: boolean
  untaggedCount: number
  totalCount: number
  bulkMode: boolean
  bulkSelectedCount: number
  viewMode: 'list' | 'cards'
  onSortChange: (value: 'updated' | 'name') => void
  onSearchChange: (value: string) => void
  onScopeFilterChange: (value: 'all' | 'global' | 'project') => void
  onToolFilterChange: (value: string) => void
  onToggleTag: (tagId: number) => void
  onToggleUntagged: () => void
  onClearTags: () => void
  onManageTags: () => void
  onToggleBulkMode: () => void
  onViewModeChange: (value: 'list' | 'cards') => void
  t: TFunction
}

const FilterBar = ({
  sortBy,
  searchQuery,
  scopeFilter,
  toolFilter,
  toolOptions,
  tags,
  selectedTagIds,
  includeUntagged,
  untaggedCount,
  totalCount,
  bulkMode,
  bulkSelectedCount,
  viewMode,
  onSortChange,
  onSearchChange,
  onScopeFilterChange,
  onToolFilterChange,
  onToggleTag,
  onToggleUntagged,
  onClearTags,
  onManageTags,
  onToggleBulkMode,
  onViewModeChange,
  t,
}: FilterBarProps) => {
  const [tagMenuOpen, setTagMenuOpen] = useState(false)
  const [tagQuery, setTagQuery] = useState('')
  const tagMenuRef = useRef<HTMLDivElement | null>(null)
  const scopeOptions: { value: 'all' | 'global' | 'project'; label: string }[] = [
    { value: 'all', label: t('scope.all') },
    { value: 'global', label: t('scope.global') },
    { value: 'project', label: t('scope.project') },
  ]
  const selectedTagSet = useMemo(() => new Set(selectedTagIds), [selectedTagIds])
  const selectedCount = selectedTagIds.length + (includeUntagged ? 1 : 0)
  const filteredTags = useMemo(() => {
    const query = tagQuery.trim().toLowerCase()
    if (!query) return tags
    return tags.filter((tag) => tag.name.toLowerCase().includes(query))
  }, [tagQuery, tags])

  useEffect(() => {
    if (!tagMenuOpen) return
    const handlePointerDown = (event: MouseEvent) => {
      if (!tagMenuRef.current?.contains(event.target as Node)) {
        setTagMenuOpen(false)
      }
    }
    document.addEventListener('mousedown', handlePointerDown)
    return () => document.removeEventListener('mousedown', handlePointerDown)
  }, [tagMenuOpen])

  return (
    <div className="filter-bar">
      <div className="filter-title">
        {t('allSkills')}（{totalCount}）
      </div>
      <div className="filter-actions">
        <button
          className={`btn btn-secondary sort-btn app-filter-btn${toolFilter !== 'all' ? ' active' : ''}`}
          type="button"
          title={t('appFilter')}
        >
          <Layers size={13} style={{ marginRight: 4, flexShrink: 0 }} />
          {toolFilter === 'all'
            ? `${t('allApps')} (${totalCount})`
            : `${toolOptions.find((t) => t.id === toolFilter)?.label ?? toolFilter} (${toolOptions.find((t) => t.id === toolFilter)?.count ?? 0})`}
          <ChevronDown size={12} />
          <select
            aria-label={t('appFilter')}
            value={toolFilter}
            onChange={(event) => onToolFilterChange(event.target.value)}
          >
            <option value="all">
              {t('allApps')} ({totalCount})
            </option>
            {toolOptions
              .filter((option) => option.count > 0)
              .map((option) => (
                <option key={option.id} value={option.id}>
                  {option.label} ({option.count})
                </option>
              ))}
          </select>
        </button>

        <button className="btn btn-secondary sort-btn" type="button">
          {scopeOptions.find((option) => option.value === scopeFilter)?.label ?? t('scope.all')}
          <ChevronDown size={12} />
          <select
            aria-label={t('scope.filterLabel')}
            value={scopeFilter}
            onChange={(event) =>
              onScopeFilterChange(event.target.value as 'all' | 'global' | 'project')
            }
          >
            {scopeOptions.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </button>
        <button className="btn btn-secondary sort-btn" type="button">
          {sortBy === 'updated' ? t('sortUpdated') : t('sortName')}
          <ArrowUpDown size={12} />
          <select
            aria-label={t('filterSort')}
            value={sortBy}
            onChange={(event) => onSortChange(event.target.value as 'updated' | 'name')}
          >
            <option value="updated">{t('sortUpdated')}</option>
            <option value="name">{t('sortName')}</option>
          </select>
        </button>
        <button
          className={`btn btn-secondary bulk-mode-btn${bulkMode ? ' active' : ''}`}
          type="button"
          onClick={onToggleBulkMode}
        >
          <CheckSquare size={14} />
          {bulkMode
            ? t('bulk.selectedShort', { count: bulkSelectedCount })
            : t('bulk.manage')}
        </button>
        <div className="tag-filter-wrap" ref={tagMenuRef}>
          <button
            className={`btn btn-secondary tag-filter-btn${selectedCount > 0 ? ' active' : ''}`}
            type="button"
            onClick={() => setTagMenuOpen((open) => !open)}
          >
            <Tags size={14} />
            {selectedCount > 0
              ? t('tagsSelected', { count: selectedCount })
              : t('tags')}
            <ChevronDown size={12} />
          </button>
          {tagMenuOpen ? (
            <div className="tag-filter-menu">
              <div className="tag-filter-head">
                <span>{t('tags')}</span>
                <span>{t('matchAny')}</span>
              </div>
              <div className="tag-filter-search">
                <Search size={15} />
                <input
                  value={tagQuery}
                  onChange={(event) => setTagQuery(event.target.value)}
                  placeholder={t('searchTags')}
                />
              </div>
              <div className="tag-filter-options">
                <button
                  className={`tag-filter-option${includeUntagged ? ' selected' : ''}`}
                  type="button"
                  onClick={onToggleUntagged}
                >
                  <span className="tag-check">{includeUntagged ? <Check size={14} /> : null}</span>
                  <span>{t('untagged')}</span>
                  <span className="tag-count">{untaggedCount}</span>
                </button>
                {filteredTags.map((tag) => {
                  const selected = selectedTagSet.has(tag.id)
                  return (
                    <button
                      key={tag.id}
                      className={`tag-filter-option${selected ? ' selected' : ''}`}
                      type="button"
                      onClick={() => onToggleTag(tag.id)}
                    >
                      <span className="tag-check">{selected ? <Check size={14} /> : null}</span>
                      <span>{tag.name}</span>
                      <span className="tag-count">{tag.skill_count}</span>
                    </button>
                  )
                })}
              </div>
              <div className="tag-filter-footer">
                <button type="button" onClick={onClearTags} disabled={selectedCount === 0}>
                  {t('clearAll')}
                </button>
                <button type="button" onClick={onManageTags}>
                  {t('manageTags')}
                </button>
              </div>
            </div>
          ) : null}
        </div>
        <div className="view-mode-toggle" role="group" aria-label={t('viewMode.label')}>
          <button
            className={viewMode === 'list' ? 'active' : ''}
            type="button"
            onClick={() => onViewModeChange('list')}
            aria-label={t('viewMode.list')}
            title={t('viewMode.list')}
            aria-pressed={viewMode === 'list'}
          >
            <List size={15} />
          </button>
          <button
            className={viewMode === 'cards' ? 'active' : ''}
            type="button"
            onClick={() => onViewModeChange('cards')}
            aria-label={t('viewMode.cards')}
            title={t('viewMode.cards')}
            aria-pressed={viewMode === 'cards'}
          >
            <LayoutGrid size={15} />
          </button>
        </div>
        <div className="search-container">
          <Search size={16} className="search-icon-abs" />
          <input
            className="search-input"
            value={searchQuery}
            onChange={(event) => onSearchChange(event.target.value)}
            placeholder={t('searchPlaceholder')}
          />
        </div>
      </div>
    </div>
  )
}

export default memo(FilterBar)
