import { memo, useState } from 'react'
import { SlidersHorizontal } from 'lucide-react'
import type { TFunction } from 'i18next'
import {
  buildDiscoverySourceEnabledMap,
  collectDisabledDiscoverySourceKeys,
} from '../discoveryScanSettings'
import type { DiscoveryScanSettingsDto } from '../types'

type DiscoveryScanModalProps = {
  open: boolean
  saving: boolean
  settings: DiscoveryScanSettingsDto | null
  onRequestClose: () => void
  onSave: (disabledSourceKeys: string[]) => void
  t: TFunction
}

const DiscoveryScanModalContent = ({
  saving,
  settings,
  onRequestClose,
  onSave,
  t,
}: Omit<DiscoveryScanModalProps, 'open'>) => {
  const [enabledByKey, setEnabledByKey] = useState<Record<string, boolean>>(() =>
    buildDiscoverySourceEnabledMap(settings),
  )

  const enabledCount = settings?.sources.filter((source) => enabledByKey[source.key]).length ?? 0
  const totalCount = settings?.sources.length ?? 0

  return (
    <div className="modal-backdrop" onClick={onRequestClose}>
      <div
        className="modal discovery-scan-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="discovery-scan-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-header">
          <div>
            <div className="modal-title" id="discovery-scan-title">
              {t('discoveryScan.title')}
            </div>
            <div className="discovery-scan-description">
              {t('discoveryScan.description')}
            </div>
          </div>
          <button
            className="modal-close"
            type="button"
            onClick={onRequestClose}
            aria-label={t('close')}
            disabled={saving}
          >
            ✕
          </button>
        </div>
        <div className="modal-body discovery-scan-body">
          <div className="discovery-scan-summary">
            <span>{t('discoveryScan.sources')}</span>
            <strong>{t('discoveryScan.enabledCount', { enabled: enabledCount, total: totalCount })}</strong>
          </div>
          <div className="discovery-scan-list">
            {settings?.sources.length ? (
              settings.sources.map((source) => {
                const enabled = Boolean(enabledByKey[source.key])
                const sourceLabel = source.key === 'claude_plugins'
                  ? t('discoveryScan.claudePlugins')
                  : source.label
                return (
                  <div
                    className="discovery-scan-source"
                    key={source.key}
                    style={{ cursor: 'pointer' }}
                    onClick={() => {
                      if (!saving) {
                        setEnabledByKey((current) => ({
                          ...current,
                          [source.key]: !enabled,
                        }))
                      }
                    }}
                  >
                    <div className="discovery-scan-source-icon">
                      <SlidersHorizontal size={16} />
                    </div>
                    <div className="discovery-scan-source-copy">
                      <strong>{sourceLabel}</strong>
                      <span className="mono" title={source.path}>{source.path}</span>
                    </div>
                    <button
                      className={`settings-toggle${enabled ? ' checked' : ''}`}
                      type="button"
                      role="switch"
                      aria-checked={enabled}
                      aria-label={t('discoveryScan.toggleSource', { source: sourceLabel })}
                      onClick={(e) => {
                        e.stopPropagation()
                        setEnabledByKey((current) => ({
                          ...current,
                          [source.key]: !enabled,
                        }))
                      }}
                      disabled={saving}
                    >
                      <span className="settings-toggle-knob" />
                    </button>
                  </div>
                )
              })
            ) : (
              <div className="empty">{t('discoveryScan.empty')}</div>
            )}
          </div>
          <div className="discovery-scan-helper">{t('discoveryScan.validOnly')}</div>
        </div>
        <div className="modal-footer">
          <button className="btn btn-secondary" type="button" onClick={onRequestClose} disabled={saving}>
            {t('cancel')}
          </button>
          <button
            className="btn btn-primary"
            type="button"
            disabled={saving || !settings}
            onClick={() => {
              if (settings) {
                onSave(collectDisabledDiscoverySourceKeys(settings, enabledByKey))
              }
            }}
          >
            {saving ? t('discoveryScan.saving') : t('discoveryScan.saveAndRescan')}
          </button>
        </div>
      </div>
    </div>
  )
}

const DiscoveryScanModal = ({ open, ...props }: DiscoveryScanModalProps) => {
  if (!open) return null

  const settingsKey =
    props.settings?.sources.map((source) => `${source.key}:${source.enabled}`).join('|') ?? 'loading'

  return <DiscoveryScanModalContent key={settingsKey} {...props} />
}

export default memo(DiscoveryScanModal)
