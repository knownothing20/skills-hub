import { useCallback, useEffect, useState, type Dispatch, type SetStateAction } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { isTauri, invokeTauri } from './useTauriInvoke'

export type ThemePreference = 'system' | 'light' | 'dark'
export type SystemTheme = 'light' | 'dark'
export type SyncMode = 'junction' | 'copy'
export type SkillViewMode = 'list' | 'cards'

export const LANGUAGE_STORAGE_KEY = 'skills-language'
export const THEME_STORAGE_KEY = 'skills-theme'
export const SKILL_VIEW_MODE_STORAGE_KEY = 'skills-view-mode'
export const SIDEBAR_COLLAPSED_STORAGE_KEY = 'skills-sidebar-collapsed'

export interface AppSettingsHook {
  language: string
  toggleLanguage: () => void
  themePreference: ThemePreference
  setThemePreference: Dispatch<SetStateAction<ThemePreference>>
  systemTheme: SystemTheme
  resolvedTheme: SystemTheme
  defaultSyncMode: SyncMode
  setDefaultSyncMode: Dispatch<SetStateAction<SyncMode>>
  handleDefaultSyncModeChange: (nextMode: SyncMode) => Promise<void>
  appVersion: string
  skillViewMode: SkillViewMode
  setSkillViewMode: Dispatch<SetStateAction<SkillViewMode>>
  sidebarCollapsed: boolean
  setSidebarCollapsed: Dispatch<SetStateAction<boolean>>
}

export function useAppSettings(): AppSettingsHook {
  const { t, i18n } = useTranslation()
  const language = i18n.resolvedLanguage ?? i18n.language ?? 'en'

  const toggleLanguage = useCallback(() => {
    void i18n.changeLanguage(language === 'en' ? 'zh' : 'en')
  }, [i18n, language])

  useEffect(() => {
    if (typeof document !== 'undefined') {
      document.documentElement.lang = language.startsWith('zh') ? 'zh-CN' : 'en'
    }
  }, [language])

  useEffect(() => {
    if (typeof window === 'undefined') return
    if (language !== 'en' && language !== 'zh') return
    try {
      window.localStorage.setItem(LANGUAGE_STORAGE_KEY, language)
    } catch {
      // ignore storage failures
    }
  }, [language])

  const [themePreference, setThemePreference] = useState<ThemePreference>('system')
  const [systemTheme, setSystemTheme] = useState<SystemTheme>('light')
  const [defaultSyncMode, setDefaultSyncMode] = useState<SyncMode>('copy')
  const [appVersion, setAppVersion] = useState('')

  const [skillViewMode, setSkillViewMode] = useState<SkillViewMode>(() => {
    if (typeof window !== 'undefined') {
      try {
        return window.localStorage.getItem(SKILL_VIEW_MODE_STORAGE_KEY) === 'cards'
          ? 'cards'
          : 'list'
      } catch {
        // ignore
      }
    }
    return 'list'
  })

  const [sidebarCollapsed, setSidebarCollapsed] = useState<boolean>(() => {
    if (typeof window !== 'undefined') {
      try {
        return window.localStorage.getItem(SIDEBAR_COLLAPSED_STORAGE_KEY) === 'true'
      } catch {
        // ignore
      }
    }
    return false
  })

  useEffect(() => {
    try {
      window.localStorage.setItem(SKILL_VIEW_MODE_STORAGE_KEY, skillViewMode)
    } catch {
      // ignore storage failures
    }
  }, [skillViewMode])

  useEffect(() => {
    try {
      window.localStorage.setItem(SIDEBAR_COLLAPSED_STORAGE_KEY, String(sidebarCollapsed))
    } catch {
      // ignore storage failures
    }
  }, [sidebarCollapsed])

  useEffect(() => {
    if (!isTauri) return
    let active = true
    void import('@tauri-apps/api/app')
      .then(({ getVersion }) => getVersion())
      .then((version) => {
        if (active) setAppVersion(version)
      })
      .catch(() => undefined)
    return () => {
      active = false
    }
  }, [])

  useEffect(() => {
    if (typeof window === 'undefined') return
    try {
      const stored = window.localStorage.getItem(THEME_STORAGE_KEY)
      if (stored === 'light' || stored === 'dark' || stored === 'system') {
        setThemePreference(stored)
      }
    } catch {
      // ignore
    }
  }, [])

  useEffect(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const update = () => {
      setSystemTheme(media.matches ? 'dark' : 'light')
    }
    update()
    if (media.addEventListener) {
      media.addEventListener('change', update)
    } else {
      media.addListener(update)
    }
    return () => {
      if (media.removeEventListener) {
        media.removeEventListener('change', update)
      } else {
        media.removeListener(update)
      }
    }
  }, [])

  const resolvedTheme: SystemTheme =
    themePreference === 'system' ? systemTheme : themePreference

  useEffect(() => {
    if (typeof document === 'undefined') return
    document.documentElement.dataset.theme = resolvedTheme
    document.documentElement.style.colorScheme = resolvedTheme
    try {
      window.localStorage.setItem(THEME_STORAGE_KEY, themePreference)
    } catch {
      // ignore storage failures
    }
  }, [resolvedTheme, themePreference])

  useEffect(() => {
    if (!isTauri) return
    invokeTauri<string>('get_default_sync_mode')
      .then((mode) => {
        if (mode === 'copy' || mode === 'junction') {
          setDefaultSyncMode(mode)
        }
      })
      .catch((err) => {
        console.warn('Failed to load default_sync_mode', err)
      })
  }, [])

  const handleDefaultSyncModeChange = useCallback(
    async (nextMode: SyncMode) => {
      setDefaultSyncMode(nextMode)
      if (isTauri) {
        try {
          await invokeTauri('set_default_sync_mode', { mode: nextMode })
          toast.success(t('syncModeUpdatedToast'))
        } catch (err) {
          toast.error(err instanceof Error ? err.message : String(err))
        }
      }
    },
    [t],
  )

  return {
    language,
    toggleLanguage,
    themePreference,
    setThemePreference,
    systemTheme,
    resolvedTheme,
    defaultSyncMode,
    setDefaultSyncMode,
    handleDefaultSyncModeChange,
    appVersion,
    skillViewMode,
    setSkillViewMode,
    sidebarCollapsed,
    setSidebarCollapsed,
  }
}
