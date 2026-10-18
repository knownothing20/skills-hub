import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { isTauri, invokeTauri } from './useTauriInvoke'
import {
  getAutoUpdateToastKey,
  shouldKeepWaitingForTriggeredAutoUpdate,
} from '../components/skills/autoUpdateSettings'
import type { AutoUpdateConfigDto } from '../components/skills/types'

export interface UseAutoUpdateOptions {
  activeView: string
  managementTab: string
  onSkillsReload: () => Promise<void>
  onSuccessToast: (msg: string) => void
  onError: (err: string) => void
}

export interface UseAutoUpdateReturn {
  autoUpdateConfig: AutoUpdateConfigDto | null
  setAutoUpdateConfig: React.Dispatch<React.SetStateAction<AutoUpdateConfigDto | null>>
  autoUpdateTriggering: boolean
  handleAutoUpdateConfigChange: (
    enabled: boolean,
    schedule: {
      scheduleType: 'interval' | 'daily'
      intervalValue: number
      intervalUnit: 'minutes' | 'hours'
      dailyTime: string
    },
  ) => Promise<void>
  handleTriggerAutoUpdateTaskNow: () => Promise<void>
}

export function useAutoUpdate({
  activeView,
  managementTab,
  onSkillsReload,
  onSuccessToast,
  onError,
}: UseAutoUpdateOptions): UseAutoUpdateReturn {
  const { t } = useTranslation()
  const [autoUpdateConfig, setAutoUpdateConfig] =
    useState<AutoUpdateConfigDto | null>(null)
  const [autoUpdateTriggering, setAutoUpdateTriggering] = useState(false)
  const autoUpdateLastRunRef = useRef<number | null>(null)

  // 初始加载一次
  useEffect(() => {
    if (!isTauri) return
    invokeTauri<AutoUpdateConfigDto>('get_auto_update_config')
      .then((config) => setAutoUpdateConfig(config))
      .catch((err) => {
        onError(err instanceof Error ? err.message : String(err))
      })
  }, [onError])

  // 处于更新管理面板时开启轮询
  useEffect(() => {
    if (!isTauri) return
    if (activeView !== 'manage' || managementTab !== 'updates') return

    let cancelled = false
    const refreshAutoUpdateConfig = async () => {
      const config = await invokeTauri<AutoUpdateConfigDto>('get_auto_update_config')
      if (cancelled) return

      const previousLastRun = autoUpdateLastRunRef.current
      const nextLastRun = config.last_run_at ?? null
      autoUpdateLastRunRef.current = nextLastRun
      setAutoUpdateConfig(config)

      if (
        previousLastRun !== null &&
        nextLastRun !== null &&
        nextLastRun !== previousLastRun &&
        config.last_status !== 'running'
      ) {
        await onSkillsReload()
      }
    }

    void refreshAutoUpdateConfig().catch(() => {})
    const timer = window.setInterval(() => {
      void refreshAutoUpdateConfig().catch(() => {})
    }, autoUpdateConfig?.last_status === 'running' ? 2000 : 10000)

    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
  }, [
    activeView,
    managementTab,
    autoUpdateConfig?.last_status,
    onSkillsReload,
  ])

  const handleAutoUpdateConfigChange = useCallback(
    async (
      enabled: boolean,
      schedule: {
        scheduleType: 'interval' | 'daily'
        intervalValue: number
        intervalUnit: 'minutes' | 'hours'
        dailyTime: string
      },
    ) => {
      const normalizedIntervalValue =
        schedule.intervalUnit === 'minutes'
          ? Math.max(15, Math.min(schedule.intervalValue, 24 * 30 * 60))
          : Math.max(1, Math.min(schedule.intervalValue, 24 * 30))
      const normalizedDailyTime = /^\d{2}:\d{2}$/.test(schedule.dailyTime)
        ? schedule.dailyTime
        : '03:00'
      const normalizedIntervalHours =
        schedule.scheduleType === 'daily'
          ? 24
          : schedule.intervalUnit === 'minutes'
            ? Math.max(1, Math.ceil(normalizedIntervalValue / 60))
            : normalizedIntervalValue
      const previousEnabled = autoUpdateConfig?.enabled
      setAutoUpdateConfig((prev) => ({
        enabled,
        interval_hours: normalizedIntervalHours,
        schedule_type: schedule.scheduleType,
        interval_value: normalizedIntervalValue,
        interval_unit: schedule.intervalUnit,
        daily_time: normalizedDailyTime,
        local_skill_count: prev?.local_skill_count ?? 0,
        protected_local_skill_count: prev?.protected_local_skill_count ?? 0,
        task_registered: prev?.task_registered ?? false,
        task_status_detail: prev?.task_status_detail ?? '',
        last_run_at: prev?.last_run_at ?? null,
        last_started_at: prev?.last_started_at ?? null,
        last_finished_at: prev?.last_finished_at ?? null,
        last_status: prev?.last_status ?? null,
        last_error: prev?.last_error ?? null,
        last_checked: prev?.last_checked ?? 0,
        last_updated: prev?.last_updated ?? 0,
        last_failed: prev?.last_failed ?? 0,
        progress: prev?.progress ?? {
          total: 0,
          succeeded: [],
          skipped: [],
          failed: [],
          running: null,
          pending: [],
        },
      }))
      if (!isTauri) return
      try {
        const updated = await invokeTauri<AutoUpdateConfigDto>(
          'set_auto_update_config',
          {
            enabled,
            intervalHours: normalizedIntervalHours,
            scheduleType: schedule.scheduleType,
            intervalValue: normalizedIntervalValue,
            intervalUnit: schedule.intervalUnit,
            dailyTime: normalizedDailyTime,
          },
        )
        setAutoUpdateConfig(updated)
        onSuccessToast(t(getAutoUpdateToastKey(previousEnabled, enabled)))
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err))
        void invokeTauri<AutoUpdateConfigDto>('get_auto_update_config')
          .then((config) => setAutoUpdateConfig(config))
          .catch(() => {})
      }
    },
    [autoUpdateConfig?.enabled, onError, onSuccessToast, t],
  )

  const handleTriggerAutoUpdateTaskNow = useCallback(async () => {
    if (!isTauri || autoUpdateTriggering) return
    const triggeredAt = Date.now()
    setAutoUpdateTriggering(true)
    setAutoUpdateConfig((prev) =>
      prev
        ? {
            ...prev,
            last_run_at: triggeredAt,
            last_started_at: triggeredAt,
            last_finished_at: null,
            last_status: 'running',
            last_error: null,
            last_checked: 0,
            last_updated: 0,
            last_failed: 0,
            progress: {
              total: 0,
              succeeded: [],
              skipped: [],
              failed: [],
              running: null,
              pending: [],
            },
          }
        : prev,
    )
    try {
      await invokeTauri('trigger_auto_update_task_now_cmd')
      onSuccessToast(t('autoUpdateTaskTriggered'))
      let sawRunning = false
      for (let attempt = 0; attempt < 30; attempt += 1) {
        await new Promise((resolve) => window.setTimeout(resolve, 2000))
        const latestConfig = await invokeTauri<AutoUpdateConfigDto>(
          'get_auto_update_config',
        )
        if (latestConfig.last_status === 'running') {
          sawRunning = true
        }
        const keepWaiting = shouldKeepWaitingForTriggeredAutoUpdate(
          latestConfig,
          triggeredAt,
          sawRunning,
        )
        if (keepWaiting && latestConfig.last_status !== 'running') {
          continue
        }
        setAutoUpdateConfig(latestConfig)
        if (!keepWaiting) {
          await onSkillsReload()
          break
        }
      }
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err))
    } finally {
      setAutoUpdateTriggering(false)
    }
  }, [autoUpdateTriggering, onError, onSkillsReload, onSuccessToast, t])

  return {
    autoUpdateConfig,
    setAutoUpdateConfig,
    autoUpdateTriggering,
    handleAutoUpdateConfigChange,
    handleTriggerAutoUpdateTaskNow,
  }
}
