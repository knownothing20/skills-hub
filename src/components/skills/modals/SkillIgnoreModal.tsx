import { memo, useCallback, useEffect, useState } from 'react'
import { Folder, File, Plus, X, ShieldAlert, Check } from 'lucide-react'
import type { TFunction } from 'i18next'
import { toast } from 'sonner'
import type { ManagedSkill } from '../types'

type SkillIgnoreItem = {
  name: string
  path: string
  is_dir: boolean
  is_ignored: boolean
}

type SkillIgnoreConfigDto = {
  rules: string[]
  items: SkillIgnoreItem[]
}

type SkillIgnoreModalProps = {
  open: boolean
  skill: ManagedSkill
  invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>
  onClose: () => void
  onSaved?: () => void
  t: TFunction
}

const SkillIgnoreModal = ({
  open,
  skill,
  invokeTauri,
  onClose,
  onSaved,
  t,
}: SkillIgnoreModalProps) => {
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [items, setItems] = useState<SkillIgnoreItem[]>([])
  const [rules, setRules] = useState<string[]>([])
  const [newRuleInput, setNewRuleInput] = useState('')

  // 加载技能的屏蔽规则与子项
  const loadConfig = useCallback(async () => {
    setLoading(true)
    try {
      const data = await invokeTauri<SkillIgnoreConfigDto>('get_skill_ignore_config', {
        skillId: skill.id,
        centralPath: skill.central_path,
      })
      setItems(data.items)

      // 提取所有生效的规则（包括后端返回的 rules 与根据现有文件判断的规则）
      const initialRules: string[] = []
      data.rules.forEach((r) => {
        const trimmed = r.trim()
        if (trimmed && !initialRules.includes(trimmed)) {
          initialRules.push(trimmed)
        }
      })

      // 结合已标记为 is_ignored 的文件/目录
      data.items.forEach((item) => {
        if (item.is_ignored) {
          const ruleCandidate = item.is_dir ? `${item.name}/` : item.name
          const cleanItem = item.name.replace(/\/$/, '')
          const alreadyMatched = initialRules.some((r) => {
            const cleanR = r.trim().replace(/\/$/, '')
            return cleanR === cleanItem
          })
          if (!alreadyMatched) {
            initialRules.push(ruleCandidate)
          }
        }
      })

      setRules(initialRules)
    } catch (err) {
      toast.error(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }, [invokeTauri, skill.id, skill.central_path])

  useEffect(() => {
    if (open) {
      void loadConfig()
    }
  }, [open, loadConfig])

  // 判断某个子文件夹/文件是否匹配当前规则
  const isItemIgnored = useCallback(
    (item: SkillIgnoreItem) => {
      const cleanItem = item.name.trim().replace(/\/$/, '')
      return rules.some((rule) => {
        const cleanRule = rule.trim().replace(/\/$/, '')
        if (cleanRule === cleanItem || cleanRule === item.path.replace(/\/$/, '')) {
          return true
        }
        if (cleanRule.startsWith('*') && cleanItem.endsWith(cleanRule.slice(1))) {
          return true
        }
        if (cleanRule.endsWith('*') && cleanItem.startsWith(cleanRule.slice(0, -1))) {
          return true
        }
        return false
      })
    },
    [rules],
  )

  // 切换上方项目的屏蔽状态（与下方规则列表实时双向联动）
  const handleToggleItem = (item: SkillIgnoreItem) => {
    const cleanItem = item.name.trim().replace(/\/$/, '')
    const isCurrentlyIgnored = isItemIgnored(item)

    if (isCurrentlyIgnored) {
      // 取消屏蔽：从 rules 中移除匹配这一项的具体规则
      setRules((prev) =>
        prev.filter((r) => {
          const cleanR = r.trim().replace(/\/$/, '')
          return cleanR !== cleanItem && cleanR !== item.path.replace(/\/$/, '')
        }),
      )
    } else {
      // 勾选屏蔽：追加对应规则（文件夹带 /，文件不带 /）
      const ruleToAdd = item.is_dir ? `${cleanItem}/` : cleanItem
      setRules((prev) => {
        if (!prev.includes(ruleToAdd)) {
          return [...prev, ruleToAdd]
        }
        return prev
      })
    }
  }

  // 添加自定义规则
  const handleAddCustomRule = () => {
    const trimmed = newRuleInput.trim()
    if (!trimmed) return
    if (!rules.includes(trimmed)) {
      setRules((prev) => [...prev, trimmed])
    }
    setNewRuleInput('')
  }

  // 移除单条规则（上方对应项会自动取消勾选）
  const handleRemoveRule = (ruleToRemove: string) => {
    setRules((prev) => prev.filter((r) => r !== ruleToRemove))
  }

  // 保存规则
  const handleSave = async () => {
    setSaving(true)
    try {
      await invokeTauri('save_skill_ignore_config', {
        skillId: skill.id,
        centralPath: skill.central_path,
        rules,
      })

      toast.success(t('detail.ignoreSavedSuccess'))
      onSaved?.()
      onClose()
    } catch (err) {
      toast.error(err instanceof Error ? err.message : String(err))
    } finally {
      setSaving(false)
    }
  }

  if (!open) return null

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal modal-lg skill-ignore-modal"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
        style={{ maxWidth: '640px' }}
      >
        <div className="modal-header">
          <div>
            <div className="modal-title" style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
              <ShieldAlert size={20} style={{ color: 'var(--color-primary, #2563eb)' }} />
              {t('detail.ignoreSettings')}
            </div>
            <div className="modal-subtitle" style={{ fontSize: '12px', color: 'var(--color-text-secondary, #64748b)', marginTop: '4px' }}>
              {t('detail.ignoreSettingsDesc')}
            </div>
          </div>
          <button
            className="modal-close"
            type="button"
            onClick={onClose}
            aria-label={t('close')}
            disabled={saving}
          >
            ✕
          </button>
        </div>

        <div className="modal-body" style={{ maxHeight: '60vh', overflowY: 'auto', padding: '16px 24px' }}>
          {loading ? (
            <div style={{ padding: '32px 0', textAlign: 'center', color: 'var(--color-text-secondary)' }}>
              {t('detail.loadingFiles')}
            </div>
          ) : (
            <>
              {/* 快速勾选区 */}
              <div style={{ marginBottom: '20px' }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '8px' }}>
                  <div style={{ fontSize: '13px', fontWeight: 600, color: 'var(--color-text-primary)' }}>
                    {t('detail.ignoreFolderListTitle')}
                  </div>
                  <div style={{ fontSize: '12px', color: 'var(--color-text-secondary)' }}>
                    点击卡片即可快速添加/取消屏蔽
                  </div>
                </div>

                {items.length === 0 ? (
                  <div style={{ fontSize: '12px', color: 'var(--color-text-secondary)', padding: '12px', background: 'var(--color-bg-secondary, rgba(0,0,0,0.02))', borderRadius: '6px' }}>
                    {t('detail.noFiles')}
                  </div>
                ) : (
                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(170px, 1fr))', gap: '8px' }}>
                    {items.map((item) => {
                      const isIgnored = isItemIgnored(item)
                      return (
                        <div
                          key={item.path}
                          onClick={() => handleToggleItem(item)}
                          style={{
                            display: 'flex',
                            alignItems: 'center',
                            gap: '8px',
                            padding: '8px 10px',
                            borderRadius: '6px',
                            border: isIgnored ? '1px solid var(--color-primary, #2563eb)' : '1px solid var(--color-border, #e2e8f0)',
                            background: isIgnored ? 'rgba(37, 99, 235, 0.08)' : 'var(--color-bg-card, #ffffff)',
                            cursor: 'pointer',
                            userSelect: 'none',
                            transition: 'all 0.15s ease',
                          }}
                        >
                          <input
                            type="checkbox"
                            checked={isIgnored}
                            onChange={() => {}} // 由外层点击处理
                            style={{ pointerEvents: 'none' }}
                          />
                          {item.is_dir ? (
                            <Folder size={15} style={{ color: isIgnored ? 'var(--color-primary)' : '#f59e0b', flexShrink: 0 }} />
                          ) : (
                            <File size={15} style={{ color: isIgnored ? 'var(--color-primary)' : '#64748b', flexShrink: 0 }} />
                          )}
                          <span
                            title={item.name}
                            style={{
                              fontSize: '12px',
                              fontWeight: isIgnored ? 600 : 400,
                              color: isIgnored ? 'var(--color-primary, #2563eb)' : 'inherit',
                              overflow: 'hidden',
                              textOverflow: 'ellipsis',
                              whiteSpace: 'nowrap',
                            }}
                          >
                            {item.name}
                          </span>
                        </div>
                      )
                    })}
                  </div>
                )}
              </div>

              {/* 完整的已生效屏蔽规则总览区 */}
              <div>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '8px' }}>
                  <div style={{ fontSize: '13px', fontWeight: 600, color: 'var(--color-text-primary)' }}>
                    已生效屏蔽规则 ({rules.length})
                  </div>
                  <div style={{ fontSize: '11px', color: 'var(--color-text-secondary)' }}>
                    上面勾选的项与自定义规则已在此实时汇总
                  </div>
                </div>

                <div style={{ display: 'flex', gap: '8px', marginBottom: '10px' }}>
                  <input
                    type="text"
                    className="input"
                    value={newRuleInput}
                    onChange={(e) => setNewRuleInput(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') {
                        e.preventDefault()
                        handleAddCustomRule()
                      }
                    }}
                    placeholder="输入自定义规则，例如 *.key, .env*，回车添加"
                    style={{ flex: 1, height: '36px', fontSize: '12px' }}
                  />
                  <button
                    type="button"
                    className="btn btn-secondary"
                    onClick={handleAddCustomRule}
                    disabled={!newRuleInput.trim()}
                    style={{ display: 'flex', alignItems: 'center', gap: '4px', height: '36px', padding: '0 12px' }}
                  >
                    <Plus size={14} />
                    {t('detail.ignoreAddRuleButton')}
                  </button>
                </div>

                {rules.length > 0 ? (
                  <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px' }}>
                    {rules.map((rule) => (
                      <span
                        key={rule}
                        style={{
                          display: 'inline-flex',
                          alignItems: 'center',
                          gap: '6px',
                          padding: '4px 8px',
                          background: 'rgba(37, 99, 235, 0.08)',
                          border: '1px solid rgba(37, 99, 235, 0.25)',
                          borderRadius: '4px',
                          fontSize: '12px',
                          fontFamily: 'monospace',
                          color: 'var(--color-primary, #2563eb)',
                          fontWeight: 500,
                        }}
                      >
                        {rule}
                        <button
                          type="button"
                          onClick={() => handleRemoveRule(rule)}
                          style={{
                            border: 'none',
                            background: 'transparent',
                            cursor: 'pointer',
                            padding: '0',
                            display: 'flex',
                            alignItems: 'center',
                            color: '#ef4444',
                          }}
                          title={`移除规则 ${rule}`}
                          aria-label={`Remove rule ${rule}`}
                        >
                          <X size={12} />
                        </button>
                      </span>
                    ))}
                  </div>
                ) : (
                  <div style={{ fontSize: '12px', color: 'var(--color-text-secondary)', padding: '10px', background: 'var(--color-bg-secondary, rgba(0,0,0,0.02))', borderRadius: '4px' }}>
                    暂无屏蔽规则，你可以直接在上方点击文件夹卡片进行勾选，或者在此输入通配规则。
                  </div>
                )}
              </div>
            </>
          )}
        </div>

        <div className="modal-footer" style={{ display: 'flex', justifyContent: 'flex-end', gap: '10px', padding: '14px 24px' }}>
          <button
            type="button"
            className="btn btn-secondary"
            onClick={onClose}
            disabled={saving}
          >
            {t('cancel')}
          </button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={handleSave}
            disabled={saving || loading}
            style={{ display: 'flex', alignItems: 'center', gap: '6px' }}
          >
            <Check size={16} />
            {saving ? t('saving') : t('detail.ignoreSaveButton')}
          </button>
        </div>
      </div>
    </div>
  )
}

export default memo(SkillIgnoreModal)
