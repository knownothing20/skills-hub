import { memo } from 'react'
import type { TFunction } from 'i18next'

type ToggleToolConfirmModalProps = {
  open: boolean
  loading: boolean
  skillName: string
  toolLabel: string
  action: 'sync' | 'unsync'
  onRequestClose: () => void
  onConfirm: () => void
  t: TFunction
}

const ToggleToolConfirmModal = ({
  open,
  loading,
  skillName,
  toolLabel,
  action,
  onRequestClose,
  onConfirm,
  t,
}: ToggleToolConfirmModalProps) => {
  if (!open) return null

  const isUnsync = action === 'unsync'

  return (
    <div className="modal-backdrop" onClick={onRequestClose}>
      <div
        className="modal"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        style={{ maxWidth: '420px' }}
      >
        <div className="modal-header">
          <div className="modal-title">
            {isUnsync
              ? (t('toolToggle.confirmUnsyncTitle', { defaultValue: '确认取消同步' }))
              : (t('toolToggle.confirmSyncTitle', { defaultValue: '确认同步技能' }))}
          </div>
        </div>
        <div className="modal-body" style={{ lineHeight: '1.6', fontSize: '14px', color: 'var(--text-secondary)' }}>
          {isUnsync ? (
            <div>
              确定要将技能 <strong style={{ color: 'var(--text-primary)' }}>{skillName}</strong> 从{' '}
              <strong style={{ color: 'var(--text-primary)' }}>{toolLabel}</strong> 中取消同步吗？
              <div style={{ marginTop: '8px', fontSize: '13px', color: 'var(--text-tertiary)' }}>
                取消后该软件将不再加载此技能。
              </div>
            </div>
          ) : (
            <div>
              确定要将技能 <strong style={{ color: 'var(--text-primary)' }}>{skillName}</strong> 同步安装到{' '}
              <strong style={{ color: 'var(--text-primary)' }}>{toolLabel}</strong> 吗？
            </div>
          )}
        </div>
        <div className="modal-footer" style={{ display: 'flex', justifyContent: 'flex-end', gap: '8px' }}>
          <button
            className="btn btn-secondary"
            onClick={onRequestClose}
            disabled={loading}
          >
            {t('cancel')}
          </button>
          <button
            className={`btn ${isUnsync ? 'btn-danger-solid' : 'btn-primary'}`}
            onClick={onConfirm}
            disabled={loading}
          >
            {isUnsync
              ? (t('toolToggle.confirmUnsyncAction', { defaultValue: '取消同步' }))
              : (t('toolToggle.confirmSyncAction', { defaultValue: '立即同步' }))}
          </button>
        </div>
      </div>
    </div>
  )
}

export default memo(ToggleToolConfirmModal)
