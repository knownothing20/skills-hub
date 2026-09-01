const supportedIconDataUrlPattern =
  /^data:image\/(?:svg\+xml|png|jpeg|webp);base64,[a-z\d+/]+={0,2}$/i
const brandColorPattern = /^#[a-f\d]{6}$/i

// The Rust boundary caps decoded files at 128 KiB. Keep a second, encoded-size
// guard here so malformed IPC payloads never become unbounded DOM attributes.
export const MAX_SKILL_ICON_DATA_URL_LENGTH = 180_000

export type SkillIconMetadataPresentation = {
  iconDataUrl: string
  brandColor: string | null
}

export const normalizeSkillIconDataUrl = (value: string | null | undefined): string | null => {
  if (!value || value.length > MAX_SKILL_ICON_DATA_URL_LENGTH) return null
  return supportedIconDataUrlPattern.test(value) ? value : null
}

export const normalizeSkillBrandColor = (value: string | null | undefined): string | null => {
  if (!value || !brandColorPattern.test(value)) return null
  return value.toUpperCase()
}

export const resolveSkillIconMetadataPresentation = (
  iconDataUrl: string | null | undefined,
  brandColor: string | null | undefined,
): SkillIconMetadataPresentation | null => {
  const normalizedIcon = normalizeSkillIconDataUrl(iconDataUrl)
  if (!normalizedIcon) return null

  return {
    iconDataUrl: normalizedIcon,
    brandColor: normalizeSkillBrandColor(brandColor),
  }
}
