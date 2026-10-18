import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { isTauri, invokeTauri } from './useTauriInvoke'
import type { DiscoveryScanSettingsDto } from '../components/skills/types'

export interface UseDiscoveryScanOptions {
  onError: (err: string) => void
  onPlanReload?: () => Promise<unknown>
}

export interface UseDiscoveryScanReturn {
  discoveryScanSettings: DiscoveryScanSettingsDto | null
  setDiscoveryScanSettings: React.Dispatch<React.SetStateAction<DiscoveryScanSettingsDto | null>>
  showDiscoveryScanModal: boolean
  setShowDiscoveryScanModal: React.Dispatch<React.SetStateAction<boolean>>
  discoveryScanSaving: boolean
  discoveryScanEnabledCount: number
  discoveryScanSourceCount: number
  loadDiscoveryScanSettings: () => Promise<DiscoveryScanSettingsDto | null>
  handleOpenDiscoveryScanSettings: () => void
  handleCloseDiscoveryScanSettings: () => void
  handleSaveDiscoveryScanSettings: (disabledSourceKeys: string[]) => Promise<void>
}

export function useDiscoveryScan({
  onError,
  onPlanReload,
}: UseDiscoveryScanOptions): UseDiscoveryScanReturn {
  const { t } = useTranslation()
  const [discoveryScanSettings, setDiscoveryScanSettings] =
    useState<DiscoveryScanSettingsDto | null>(null)
  const [showDiscoveryScanModal, setShowDiscoveryScanModal] = useState(false)
  const [discoveryScanSaving, setDiscoveryScanSaving] = useState(false)

  const loadDiscoveryScanSettings = useCallback(async () => {
    if (!isTauri) return null
    try {
      const result = await invokeTauri<DiscoveryScanSettingsDto>(
        'get_discovery_scan_settings',
      )
      setDiscoveryScanSettings(result)
      return result
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err))
      return null
    }
  }, [onError])

  useEffect(() => {
    void loadDiscoveryScanSettings()
  }, [loadDiscoveryScanSettings])

  const handleOpenDiscoveryScanSettings = useCallback(() => {
    setShowDiscoveryScanModal(true)
    void loadDiscoveryScanSettings()
  }, [loadDiscoveryScanSettings])

  const handleCloseDiscoveryScanSettings = useCallback(() => {
    if (!discoveryScanSaving) setShowDiscoveryScanModal(false)
  }, [discoveryScanSaving])

  const handleSaveDiscoveryScanSettings = useCallback(
    async (disabledSourceKeys: string[]) => {
      if (!isTauri) return
      setDiscoveryScanSaving(true)
      try {
        const saved = await invokeTauri<DiscoveryScanSettingsDto>(
          'set_discovery_scan_config',
          {
            config: { disabled_source_keys: disabledSourceKeys },
          },
        )
        setDiscoveryScanSettings(saved)
        if (onPlanReload) {
          await onPlanReload()
        }
        setShowDiscoveryScanModal(false)
        toast.success(t('discoveryScan.saved'), { duration: 1600 })
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err))
      } finally {
        setDiscoveryScanSaving(false)
      }
    },
    [onError, onPlanReload, t],
  )

  const discoveryScanEnabledCount = useMemo(
    () => discoveryScanSettings?.sources.filter((source) => source.enabled).length ?? 0,
    [discoveryScanSettings],
  )

  const discoveryScanSourceCount = useMemo(
    () => discoveryScanSettings?.sources.length ?? 0,
    [discoveryScanSettings],
  )

  return {
    discoveryScanSettings,
    setDiscoveryScanSettings,
    showDiscoveryScanModal,
    setShowDiscoveryScanModal,
    discoveryScanSaving,
    discoveryScanEnabledCount,
    discoveryScanSourceCount,
    loadDiscoveryScanSettings,
    handleOpenDiscoveryScanSettings,
    handleCloseDiscoveryScanSettings,
    handleSaveDiscoveryScanSettings,
  }
}
