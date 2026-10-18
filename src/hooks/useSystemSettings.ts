import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { isTauri, invokeTauri } from './useTauriInvoke'
import type {
  GithubProxyConfigDto,
  ToolConfigDto,
  ToolStatusDto,
} from '../components/skills/types'

export interface UseSystemSettingsProps {
  setError: (error: string | null) => void
  setSuccessToastMessage: (msg: string | null) => void
  setToolStatus: React.Dispatch<React.SetStateAction<ToolStatusDto | null>>
  setSyncTargets: React.Dispatch<React.SetStateAction<Record<string, boolean>>>
}

export function useSystemSettings({
  setError,
  setSuccessToastMessage,
  setToolStatus,
  setSyncTargets,
}: UseSystemSettingsProps) {
  const { t } = useTranslation()

  const [storagePath, setStoragePath] = useState<string>(t('notAvailable'))
  const [gitCacheTtlSecs, setGitCacheTtlSecs] = useState<number>(60)
  const [githubProxyConfig, setGithubProxyConfig] =
    useState<GithubProxyConfigDto>({
      enabled: false,
      port: 7890,
      url: '',
      auto_detected: false,
    })
  const [toolConfig, setToolConfig] = useState<ToolConfigDto | null>(null)

  useEffect(() => {
    if (!isTauri) return
    invokeTauri<string>('get_central_repo_path')
      .then((path) => setStoragePath(path))
      .catch((err) => {
        setError(err instanceof Error ? err.message : String(err))
      })
  }, [setError])

  useEffect(() => {
    if (!isTauri) return
    invokeTauri<number>('get_git_cache_ttl_secs')
      .then((secs) => setGitCacheTtlSecs(secs))
      .catch((err) => {
        setError(err instanceof Error ? err.message : String(err))
      })
  }, [setError])

  useEffect(() => {
    if (!isTauri) return
    invokeTauri<GithubProxyConfigDto>('get_github_proxy_config')
      .then((config) => setGithubProxyConfig(config))
      .catch((err) => {
        setError(err instanceof Error ? err.message : String(err))
      })
  }, [setError])

  useEffect(() => {
    if (!isTauri) return
    invokeTauri<ToolConfigDto>('get_tool_config')
      .then((config) => setToolConfig(config))
      .catch((err) => {
        setError(err instanceof Error ? err.message : String(err))
      })
  }, [setError])

  const handleGitCacheTtlSecsChange = useCallback(
    async (nextSecs: number) => {
      const normalized = Math.max(0, Math.min(nextSecs, 3600))
      setGitCacheTtlSecs(normalized)
      if (!isTauri) return
      try {
        const updated = await invokeTauri<number>('set_git_cache_ttl_secs', {
          secs: normalized,
        })
        setGitCacheTtlSecs(updated)
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      }
    },
    [setError],
  )

  const handleGithubProxyConfigChange = useCallback(
    async (enabled: boolean, port: number) => {
      const normalizedPort = Math.max(1, Math.min(Math.round(port), 65535))
      setGithubProxyConfig((prev) => ({
        ...prev,
        enabled,
        port: normalizedPort,
        url: enabled ? `http://127.0.0.1:${normalizedPort}` : '',
        auto_detected: false,
      }))
      if (!isTauri) return
      try {
        const saved = await invokeTauri<GithubProxyConfigDto>(
          'set_github_proxy_config',
          {
            enabled,
            port: normalizedPort,
          },
        )
        setGithubProxyConfig(saved)
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
        void invokeTauri<GithubProxyConfigDto>('get_github_proxy_config')
          .then((config) => setGithubProxyConfig(config))
          .catch(() => {})
      }
    },
    [setError],
  )

  const handleToolConfigChange = useCallback(
    async (nextConfig: ToolConfigDto) => {
      setToolConfig(nextConfig)
      if (!isTauri) return true
      try {
        const saved = await invokeTauri<ToolConfigDto>('set_tool_config', {
          config: nextConfig,
        })
        setToolConfig(saved)
        const status = await invokeTauri<ToolStatusDto>('get_tool_status')
        setToolStatus(status)
        setSyncTargets((prev) => {
          const installed = new Set(status.installed)
          const next: Record<string, boolean> = {}
          for (const info of status.tools) {
            next[info.key] = Boolean(prev[info.key]) && installed.has(info.key)
          }
          return next
        })
        toast.success(t('toolManagement.saved'), { duration: 1600 })
        return true
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
        try {
          const [config, status] = await Promise.all([
            invokeTauri<ToolConfigDto>('get_tool_config'),
            invokeTauri<ToolStatusDto>('get_tool_status'),
          ])
          setToolConfig(config)
          setToolStatus(status)
        } catch {
          // Preserve the original save error.
        }
        return false
      }
    },
    [setError, setSyncTargets, setToolStatus, t],
  )

  const handleClearGitCacheNow = useCallback(async () => {
    if (!isTauri) {
      setError(t('errors.notTauri'))
      return
    }
    try {
      const removed = await invokeTauri<number>('clear_git_cache_now')
      setSuccessToastMessage(t('status.gitCacheCleared', { count: removed }))
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }, [setError, setSuccessToastMessage, t])

  return {
    storagePath,
    setStoragePath,
    gitCacheTtlSecs,
    setGitCacheTtlSecs,
    githubProxyConfig,
    setGithubProxyConfig,
    toolConfig,
    setToolConfig,
    handleGitCacheTtlSecsChange,
    handleGithubProxyConfigChange,
    handleToolConfigChange,
    handleClearGitCacheNow,
  }
}
