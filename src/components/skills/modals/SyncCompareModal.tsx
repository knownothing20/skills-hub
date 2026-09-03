import { memo, useCallback, useEffect, useState } from 'react'
import { Folder, Clock, RefreshCw, ArrowUpRight, ArrowDownRight, CheckCircle2, AlertTriangle, X } from 'lucide-react'
import type { TFunction } from 'i18next'
import { toast } from 'sonner'
import type { ManagedSkill } from '../types'

export type SkillTargetComparisonDto = {
  tool: string
  tool_label: string
  target_path: string
  central_path: string
  central_mtime: number
  tool_mtime: number
  central_file_count: number
  tool_file_count: number
  status: 'synced' | 'tool_newer' | 'central_newer' | 'missing'
  diff_description: string
}

type SyncCompareModalProps = {
  open: boolean
  skill: ManagedSkill
  invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>
  onClose: () => void
  onSuccess?: () => void
  t: TFunction
}

function formatTimestamp(ms: number): string {
  if (!ms || ms <= 0) return '无'
  const d = new Date(ms)
  const pad = (n: number) => n.toString().padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

export const SyncCompareModal = memo(({
  open,
  skill,
  invokeTauri,
  onClose,
  onSuccess,
}: SyncCompareModalProps) => {
  const [loading, setLoading] = useState(true)
  const [operatingTool, setOperatingTool] = useState<string | null>(null)
  const [comparisons, setComparisons] = useState<SkillTargetComparisonDto[]>([])

  const loadData = useCallback(async () => {
    setLoading(true)
    try {
      const res = await invokeTauri<SkillTargetComparisonDto[]>('get_skill_target_comparisons', {
        skillId: skill.id,
      })
      setComparisons(res || [])
    } catch (err) {
      toast.error(`获取比对失败: ${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setLoading(false)
    }
  }, [invokeTauri, skill.id])

  useEffect(() => {
    if (open) {
      void loadData()
    }
  }, [open, loadData])

  const handlePromoteToCentral = async (tool: string, toolLabel: string) => {
    if (!window.confirm(`确定要将【${toolLabel}】中的修改反向更新为【全局母版】吗？\n\n这将会用该软件的最新代码更新 ~/.agents/skills 官方母版。`)) {
      return
    }
    setOperatingTool(tool)
    try {
      await invokeTauri('promote_target_to_central', {
        skillId: skill.id,
        tool,
      })
      toast.success(`已成功将【${toolLabel}】的代码设为全局母版！`)
      await loadData()
      onSuccess?.()
    } catch (err) {
      toast.error(`设为母版失败: ${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setOperatingTool(null)
    }
  }

  const handleSyncDown = async (tool: string, toolLabel: string) => {
    if (
      !window.confirm(
        `确定要用【母版最新代码】更新【${toolLabel}】吗？\n\n注意：如果该软件中有未设为母版的私有修改，将会被母版代码替换。`,
      )
    ) {
      return
    }
    setOperatingTool(tool)
    try {
      await invokeTauri('sync_skill_to_tool', {
        sourcePath: skill.central_path,
        skillId: skill.id,
        tool,
        name: skill.name,
        overwrite: true,
        scope: 'global',
      })
      toast.success(`已成功将母版最新代码更新至【${toolLabel}】！`)
      await loadData()
      onSuccess?.()
    } catch (err) {
      toast.error(`更新此软件失败: ${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setOperatingTool(null)
    }
  }

  if (!open) return null

  const centralMtime = comparisons[0]?.central_mtime || 0
  const centralFiles = comparisons[0]?.central_file_count || 0

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal modal-lg"
        onClick={(e) => e.stopPropagation()}
        style={{
          maxWidth: '820px',
          width: '94%',
          maxHeight: '88vh',
          display: 'flex',
          flexDirection: 'column',
          padding: 0,
          overflow: 'hidden',
          borderRadius: '12px',
          boxShadow: '0 20px 40px rgba(0,0,0,0.3)',
        }}
      >
        <div
          className="modal-header"
          style={{
            padding: '16px 20px',
            borderBottom: '1px solid var(--color-border, #e5e7eb)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
            <RefreshCw size={18} style={{ color: 'var(--color-primary, #2563eb)' }} />
            <h3 style={{ margin: 0, fontSize: '16px', fontWeight: 600 }}>
              多端对比与设为母版 - {skill.name}
            </h3>
          </div>
          <button
            type="button"
            className="btn-icon"
            onClick={onClose}
            style={{ border: 'none', background: 'transparent', cursor: 'pointer', padding: '4px' }}
          >
            <X size={18} />
          </button>
        </div>

        <div
          className="modal-body"
          style={{
            padding: '20px',
            overflowY: 'auto',
            display: 'flex',
            flexDirection: 'column',
            gap: '16px',
          }}
        >
          {/* 中心母版状态展示卡 */}
          <div
            style={{
              padding: '14px 16px',
              backgroundColor: 'rgba(37, 99, 235, 0.06)',
              border: '1px solid rgba(37, 99, 235, 0.2)',
              borderRadius: '8px',
              display: 'flex',
              flexDirection: 'column',
              gap: '6px',
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: '6px', fontWeight: 600, color: 'var(--color-primary, #2563eb)' }}>
                <Folder size={16} />
                <span>全局中心母版 (Single Source of Truth)</span>
              </div>
              <span style={{ fontSize: '12px', color: 'var(--color-text-muted, #6b7280)' }}>
                文件数: <strong>{centralFiles}</strong> 个
              </span>
            </div>
            <div style={{ fontSize: '12px', color: 'var(--color-text-secondary, #4b5563)', wordBreak: 'break-all' }}>
              路径: <code>{skill.central_path}</code>
            </div>
            <div style={{ fontSize: '12px', color: 'var(--color-text-secondary, #4b5563)', display: 'flex', alignItems: 'center', gap: '4px' }}>
              <Clock size={13} />
              <span>母版最新修改时间: <strong>{formatTimestamp(centralMtime)}</strong></span>
            </div>
          </div>

          <div style={{ fontSize: '13px', color: 'var(--color-text-muted, #6b7280)', lineHeight: '1.5' }}>
            💡 <strong>识别原理说明：</strong>系统通过实时比对各工具目录与母版的文件修改时间戳（mtime）与文件树。如果某个软件中的修改时间晚于母版，系统会自动标记为 <span style={{ color: '#d97706', fontWeight: 600 }}>【工具较新】</span>，您可以随时一键将其提拔为全局母版！
          </div>

          {/* 各端对比列表 */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
            <h4 style={{ margin: '4px 0', fontSize: '14px', fontWeight: 600 }}>关联软件与状态对比</h4>

            {loading ? (
              <div style={{ padding: '30px', textAlign: 'center', color: 'var(--color-text-muted, #6b7280)' }}>
                正在实时扫描各软件目录并比对时间戳...
              </div>
            ) : comparisons.length === 0 ? (
              <div style={{ padding: '20px', textAlign: 'center', color: 'var(--color-text-muted, #6b7280)' }}>
                当前尚未关联任何下游工具
              </div>
            ) : (
              comparisons.map((comp) => {
                const isToolNewer = comp.status === 'tool_newer'
                const isSynced = comp.status === 'synced'
                const isCentralNewer = comp.status === 'central_newer'
                const isOperating = operatingTool === comp.tool

                return (
                  <div
                    key={comp.tool}
                    style={{
                      padding: '14px',
                      borderRadius: '8px',
                      border: isToolNewer
                        ? '1px solid #f59e0b'
                        : '1px solid var(--color-border, #e5e7eb)',
                      backgroundColor: isToolNewer
                        ? 'rgba(245, 158, 11, 0.04)'
                        : 'var(--color-bg-secondary, #f9fafb)',
                      display: 'flex',
                      flexDirection: 'column',
                      gap: '8px',
                    }}
                  >
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                      <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                        <span style={{ fontWeight: 600, fontSize: '14px' }}>{comp.tool_label.toUpperCase()}</span>
                        {isSynced && (
                          <span
                            style={{
                              fontSize: '11px',
                              padding: '2px 8px',
                              borderRadius: '12px',
                              backgroundColor: 'rgba(16, 185, 129, 0.15)',
                              color: '#059669',
                              display: 'inline-flex',
                              alignItems: 'center',
                              gap: '4px',
                              fontWeight: 500,
                            }}
                          >
                            <CheckCircle2 size={12} /> 完全一致 (Synced)
                          </span>
                        )}
                        {isToolNewer && (
                          <span
                            style={{
                              fontSize: '11px',
                              padding: '2px 8px',
                              borderRadius: '12px',
                              backgroundColor: 'rgba(245, 158, 11, 0.2)',
                              color: '#d97706',
                              display: 'inline-flex',
                              alignItems: 'center',
                              gap: '4px',
                              fontWeight: 600,
                            }}
                          >
                            <AlertTriangle size={12} /> 工具修改较新 (可设为母版)
                          </span>
                        )}
                        {isCentralNewer && (
                          <span
                            style={{
                              fontSize: '11px',
                              padding: '2px 8px',
                              borderRadius: '12px',
                              backgroundColor: 'rgba(37, 99, 235, 0.12)',
                              color: '#2563eb',
                              display: 'inline-flex',
                              alignItems: 'center',
                              gap: '4px',
                              fontWeight: 500,
                            }}
                          >
                            <ArrowDownRight size={12} /> 母版较新 (可同步)
                          </span>
                        )}
                      </div>

                      {/* 两个操作按钮 */}
                      <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                        <button
                          type="button"
                          className="btn btn-secondary"
                          disabled={isOperating}
                          onClick={() => void handleSyncDown(comp.tool, comp.tool_label)}
                          style={{
                            fontSize: '12px',
                            padding: '4px 10px',
                            height: 'auto',
                            display: 'inline-flex',
                            alignItems: 'center',
                            gap: '4px',
                            borderRadius: '6px',
                            cursor: isOperating ? 'not-allowed' : 'pointer',
                          }}
                          title="用母版最新代码更新此软件"
                        >
                          <ArrowDownRight size={13} />
                          更新此软件
                        </button>

                        <button
                          type="button"
                          className="btn btn-primary"
                          disabled={isOperating}
                          onClick={() => void handlePromoteToCentral(comp.tool, comp.tool_label)}
                          style={{
                            fontSize: '12px',
                            padding: '4px 12px',
                            height: 'auto',
                            display: 'inline-flex',
                            alignItems: 'center',
                            gap: '4px',
                            borderRadius: '6px',
                            cursor: isOperating ? 'not-allowed' : 'pointer',
                            backgroundColor: isToolNewer ? '#d97706' : 'var(--color-primary, #2563eb)',
                            borderColor: isToolNewer ? '#d97706' : 'var(--color-primary, #2563eb)',
                            fontWeight: isToolNewer ? 600 : 500,
                          }}
                          title="将此工具的最新改动设为全局母版"
                        >
                          <ArrowUpRight size={13} />
                          {isOperating ? '更新中...' : '⬆️ 设为母版'}
                        </button>
                      </div>
                    </div>

                    <div style={{ fontSize: '12px', color: 'var(--color-text-secondary, #4b5563)', wordBreak: 'break-all' }}>
                      路径: <code>{comp.target_path}</code>
                    </div>

                    <div style={{ fontSize: '12px', color: 'var(--color-text-secondary, #4b5563)', display: 'flex', alignItems: 'center', gap: '16px' }}>
                      <span>文件数: <strong>{comp.tool_file_count}</strong></span>
                      <span style={{ display: 'inline-flex', alignItems: 'center', gap: '4px' }}>
                        <Clock size={12} /> 工具最后修改: <strong>{formatTimestamp(comp.tool_mtime)}</strong>
                      </span>
                      <span style={{ color: isToolNewer ? '#d97706' : 'var(--color-text-muted, #9ca3af)', fontWeight: isToolNewer ? 600 : 400 }}>
                        {comp.diff_description}
                      </span>
                    </div>
                  </div>
                )
              })
            )}
          </div>
        </div>

        <div
          className="modal-footer"
          style={{
            padding: '12px 20px',
            borderTop: '1px solid var(--color-border, #e5e7eb)',
            display: 'flex',
            justifyContent: 'flex-end',
            gap: '10px',
          }}
        >
          <button type="button" className="btn btn-secondary" onClick={onClose}>
            关闭
          </button>
        </div>
      </div>
    </div>
  )
})

SyncCompareModal.displayName = 'SyncCompareModal'
