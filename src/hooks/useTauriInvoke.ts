import { useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

/**
 * Detects whether the current window is running inside a Tauri shell.
 *
 * The check is intentionally evaluated once at module load so every
 * consumer sees a consistent, stable value across renders.
 */
export const isTauri =
  typeof window !== 'undefined' &&
  Boolean(
    (window as { __TAURI__?: unknown }).__TAURI__ ||
      (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__,
  )

/**
 * Thin, type-safe wrapper around `@tauri-apps/api/core#invoke`.
 *
 * The Tauri module is dynamically imported so the browser-only dev
 * build never hard-fails on missing native bindings.
 */
export async function invokeTauri<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!isTauri) {
    throw new Error('Tauri API is not available')
  }
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke<T>(command, args)
}

/**
 * Translates a raw backend error string into a user-facing,
 * locale-aware message.  Recognises the pipe-delimited error
 * protocol used by `commands/mod.rs` (e.g. `TARGET_EXISTS|…`).
 */
export function useFormatErrorMessage() {
  const { t } = useTranslation()

  return useCallback(
    (raw: string): string => {
      if (raw.includes('CANCELLED|')) {
        return '' // Silently ignore cancelled operations
      }
      if (raw.includes('skill already exists in central repo')) {
        const pathMatch = raw.match(/central repo:\s*"?([^"]+)"?/)
        if (pathMatch) {
          const skillName = pathMatch[1].split('/').pop() ?? ''
          if (skillName) {
            return t('errors.skillExistsInHubNamed', { name: skillName })
          }
        }
        return t('errors.skillExistsInHub')
      }
      if (raw.startsWith('TARGET_EXISTS|')) {
        return t('errors.targetExists')
      }
      if (raw.startsWith('TOOL_NOT_INSTALLED|')) {
        return t('errors.toolNotInstalled')
      }
      if (raw.startsWith('TOOL_NOT_WRITABLE|')) {
        const parts = raw.split('|')
        return t('errors.toolNotWritable', { tool: parts[1] ?? '', path: parts[2] ?? '' })
      }
      if (raw.startsWith('PROJECT_SCOPE_UNSUPPORTED|')) {
        const tool = raw.split('|')[1] ?? ''
        return t('projectSync.unsupportedTool', { tool })
      }
      if (raw.includes('未在该仓库中发现可导入的 Skills')) {
        return t('errors.noSkillsFoundInRepo')
      }
      return raw
    },
    [t],
  )
}

/**
 * Shows a toast for the first error in the array and appends a
 * "+N more" suffix when multiple errors are present.
 */
export function useShowActionErrors() {
  const { t } = useTranslation()
  const formatErrorMessage = useFormatErrorMessage()

  return useCallback(
    (errors: { title: string; message: string }[]) => {
      if (errors.length === 0) return
      const head = errors[0]
      const more =
        errors.length > 1
          ? t('errors.moreCount', { count: errors.length - 1 })
          : ''
      toast.error(
        `${formatErrorMessage(`${head.title}\n${head.message}`)}${more}`,
        { duration: 3200 },
      )
    },
    [formatErrorMessage, t],
  )
}

/** Formats a timestamp delta as a human-readable relative string. */
export function useFormatRelative() {
  const { t } = useTranslation()

  return useCallback(
    (ms: number | null | undefined): string => {
      if (!ms) return t('relative.empty')
      const diff = Date.now() - ms
      if (diff < 0) return t('relative.empty')
      const minutes = Math.floor(diff / 60000)
      if (minutes < 1) return t('relative.justNow')
      if (minutes < 60) {
        return t('relative.minutesAgo', { minutes })
      }
      const hours = Math.floor(minutes / 60)
      if (hours < 24) {
        return t('relative.hoursAgo', { hours })
      }
      const days = Math.floor(hours / 24)
      return t('relative.daysAgo', { days })
    },
    [t],
  )
}

/** Extracts owner/repo from a GitHub URL for display. */
export function getGithubInfo(url: string | null | undefined) {
  if (!url) return null
  const normalized = url.replace(/^git\+/, '')
  try {
    const parsed = new URL(normalized)
    if (!parsed.hostname.includes('github.com')) return null
    const parts = parsed.pathname.split('/').filter(Boolean)
    const owner = parts[0]
    const repo = parts[1]?.replace(/\.git$/, '')
    if (!owner || !repo) return null
    return {
      label: `${owner}/${repo}`,
      href: `https://github.com/${owner}/${repo}`,
    }
  } catch {
    const match = normalized.match(/github\.com\/([^/]+)\/([^/#?]+)/i)
    if (!match) return null
    const owner = match[1]
    const repo = match[2].replace(/\.git$/, '')
    return {
      label: `${owner}/${repo}`,
      href: `https://github.com/${owner}/${repo}`,
    }
  }
}

/** Returns a human-readable source label for a managed skill. */
export function getSkillSourceLabel(skill: { source_type: string; source_ref?: string | null; central_path: string }) {
  const key = skill.source_type.toLowerCase()
  if (key.includes('git') && skill.source_ref) {
    return skill.source_ref
  }
  return skill.central_path
}
