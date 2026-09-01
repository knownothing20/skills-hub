/// <reference types="node" />

import { readFileSync } from 'node:fs'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import SkillIcon from './SkillIcon'
import {
  MAX_SKILL_ICON_DATA_URL_LENGTH,
  normalizeSkillBrandColor,
  normalizeSkillIconDataUrl,
} from './skillIconPresentation'

const appCss = readFileSync(new URL('../../App.css', import.meta.url), 'utf8')
const skillIconSource = readFileSync(new URL('./SkillIcon.tsx', import.meta.url), 'utf8')
const pngIcon = 'data:image/png;base64,iVBORw0KGgo='
const svgIcon = 'data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciPjwvc3ZnPg=='

describe('SkillIcon', () => {
  it('prefers a Skill metadata image and brand color over the semantic fallback', () => {
    const markup = renderToStaticMarkup(
      <SkillIcon
        name="search-skill"
        iconDataUrl={pngIcon}
        brandColor="#12abef"
      />,
    )

    expect(markup).toContain('data-icon-source="metadata"')
    expect(markup).toContain('--skill-icon-color:#12ABEF')
    expect(markup.match(/<img/g)).toHaveLength(1)
    expect(markup).not.toContain('data-icon-source="search"')
    expect(markup).not.toContain('skill-icon-badge')
  })

  it('accepts the four metadata image formats emitted by the backend', () => {
    expect(normalizeSkillIconDataUrl(svgIcon)).toBe(svgIcon)
    expect(normalizeSkillIconDataUrl(pngIcon)).toBe(pngIcon)
    expect(normalizeSkillIconDataUrl('data:image/jpeg;base64,/9j/2Q==')).not.toBeNull()
    expect(normalizeSkillIconDataUrl('data:image/webp;base64,UklGRg==')).not.toBeNull()
  })

  it.each([
    'https://example.com/icon.png',
    '//example.com/icon.png',
    'data:text/html;base64,PHNjcmlwdD4=',
    'data:image/gif;base64,R0lGODlh',
    'data:image/png,not-base64',
  ])('rejects unsupported or non-data metadata image source %s', (source) => {
    expect(normalizeSkillIconDataUrl(source)).toBeNull()
  })

  it('bounds image payloads and accepts only strict six-digit brand colors', () => {
    const oversized = 'data:image/png;base64,' + 'A'.repeat(MAX_SKILL_ICON_DATA_URL_LENGTH)

    expect(normalizeSkillIconDataUrl(oversized)).toBeNull()
    expect(normalizeSkillBrandColor('#12abef')).toBe('#12ABEF')
    expect(normalizeSkillBrandColor('#abc')).toBeNull()
    expect(normalizeSkillBrandColor('red')).toBeNull()
    expect(normalizeSkillBrandColor('url(https://example.com)')).toBeNull()
  })

  it('uses a generic semantic icon when metadata is absent or rejected', () => {
    const audioMarkup = renderToStaticMarkup(
      <SkillIcon name="meeting-transcript" iconDataUrl="https://example.com/icon.png" />,
    )
    const unknownMarkup = renderToStaticMarkup(<SkillIcon name="future-capability" />)

    expect(audioMarkup).toContain('data-icon-source="waveform"')
    expect(audioMarkup).not.toContain('<img')
    expect(unknownMarkup).toContain('data-icon-source="package"')
  })

  it('fills and clips the single metadata image without overlay rules', () => {
    expect(appCss).toMatch(
      /\.skill-icon\.skill-icon-metadata\s*\{[^}]*padding:\s*0;[^}]*overflow:\s*hidden;/s,
    )
    expect(appCss).toMatch(
      /\.skill-icon-metadata img\s*\{[^}]*width:\s*100%;[^}]*height:\s*100%;[^}]*object-fit:\s*cover;/s,
    )
    expect(appCss).not.toContain('.skill-icon-badge')
    expect(appCss).not.toContain('.skill-icon-secondary-brand')
  })

  it('contains no private asset registry or hard-coded brand image imports', () => {
    expect(skillIconSource).not.toContain('assets/skill-icons')
    expect(skillIconSource).not.toContain('@lobehub/icons-static-svg')
    expect(skillIconSource).not.toContain('brandAssetByKey')
  })
})
