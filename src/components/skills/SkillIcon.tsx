import { useState, type CSSProperties } from 'react'
import {
  CalendarDots,
  ChatsCircle,
  Code,
  CurrencyCircleDollar,
  Database,
  Envelope,
  FilePdf,
  FileText,
  FilmSlate,
  FlowArrow,
  GitBranch,
  GlobeHemisphereWest,
  GraduationCap,
  ImageSquare,
  MagnifyingGlass,
  Package,
  PresentationChart,
  ShieldCheck,
  Table,
  Waveform,
  type Icon,
} from '@phosphor-icons/react'
import {
  resolveSkillIconSpec,
  type SemanticSkillIconKey,
  type ToneName,
} from './skillIconRegistry'
import { resolveSkillIconMetadataPresentation } from './skillIconPresentation'

type SkillIconProps = {
  name: string
  iconDataUrl?: string | null
  brandColor?: string | null
}

type IconStyle = CSSProperties & {
  '--skill-icon-color': string
  '--skill-icon-bg': string
}

const semanticIconByKey: Record<SemanticSkillIconKey, Icon> = {
  calendar: CalendarDots,
  chats: ChatsCircle,
  code: Code,
  currency: CurrencyCircleDollar,
  database: Database,
  envelope: Envelope,
  'file-pdf': FilePdf,
  'file-text': FileText,
  film: FilmSlate,
  flow: FlowArrow,
  'git-branch': GitBranch,
  globe: GlobeHemisphereWest,
  graduation: GraduationCap,
  image: ImageSquare,
  package: Package,
  presentation: PresentationChart,
  search: MagnifyingGlass,
  shield: ShieldCheck,
  table: Table,
  waveform: Waveform,
}

const toneByName: Record<ToneName, { color: string; background: string }> = {
  amber: { color: '#9a5b00', background: '#fff4d6' },
  blue: { color: '#2463d4', background: '#eaf1ff' },
  brown: { color: '#84512d', background: '#f8eee5' },
  cyan: { color: '#087b91', background: '#e1f8fc' },
  emerald: { color: '#087451', background: '#e5f8ef' },
  gold: { color: '#906700', background: '#fff7ce' },
  indigo: { color: '#4c51bf', background: '#eeedff' },
  lime: { color: '#4b7d12', background: '#eff9d9' },
  magenta: { color: '#a72d86', background: '#fdeafa' },
  navy: { color: '#244b73', background: '#e8f0f7' },
  orange: { color: '#b8510c', background: '#fff0e3' },
  pink: { color: '#ad3575', background: '#ffeaf5' },
  red: { color: '#b93737', background: '#ffebeb' },
  rose: { color: '#b52d57', background: '#ffe9ef' },
  sky: { color: '#196ca6', background: '#e7f5ff' },
  slate: { color: '#475569', background: '#eef2f6' },
  teal: { color: '#08736c', background: '#e2f7f5' },
  violet: { color: '#7047b8', background: '#f2eaff' },
}

const SemanticSkillIcon = ({ name, style }: { name: string; style: IconStyle }) => {
  const spec = resolveSkillIconSpec(name)
  const Icon = semanticIconByKey[spec.key]

  return (
    <div
      className="skill-icon skill-icon-semantic"
      data-icon-source={spec.key}
      style={style}
      aria-hidden="true"
    >
      <Icon size={28} weight="duotone" />
    </div>
  )
}

type MetadataSkillIconProps = {
  name: string
  iconDataUrl: string
  style: IconStyle
}

const MetadataSkillIcon = ({ name, iconDataUrl, style }: MetadataSkillIconProps) => {
  const [loadFailure, setLoadFailure] = useState<{
    name: string
    iconDataUrl: string
  } | null>(null)
  const loadFailed =
    loadFailure?.name === name && loadFailure.iconDataUrl === iconDataUrl

  if (loadFailed) return <SemanticSkillIcon name={name} style={style} />

  return (
    <div
      className="skill-icon skill-icon-metadata"
      data-icon-source="metadata"
      style={style}
      aria-hidden="true"
    >
      <img
        src={iconDataUrl}
        alt=""
        draggable={false}
        onLoad={() => setLoadFailure(null)}
        onError={() => setLoadFailure({ name, iconDataUrl })}
      />
    </div>
  )
}

const SkillIcon = ({ name, iconDataUrl, brandColor }: SkillIconProps) => {
  const fallback = resolveSkillIconSpec(name)
  const tone = toneByName[fallback.tone]
  const metadata = resolveSkillIconMetadataPresentation(iconDataUrl, brandColor)
  const style: IconStyle = {
    '--skill-icon-color': metadata?.brandColor ?? tone.color,
    '--skill-icon-bg': tone.background,
  }

  if (!metadata) return <SemanticSkillIcon name={name} style={style} />

  return (
    <MetadataSkillIcon
      name={name}
      iconDataUrl={metadata.iconDataUrl}
      style={style}
    />
  )
}

export default SkillIcon
