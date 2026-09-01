export const toneNames = [
  'amber',
  'blue',
  'brown',
  'cyan',
  'emerald',
  'gold',
  'indigo',
  'lime',
  'magenta',
  'navy',
  'orange',
  'pink',
  'red',
  'rose',
  'sky',
  'slate',
  'teal',
  'violet',
] as const

export type ToneName = (typeof toneNames)[number]

export const semanticSkillIconKeys = [
  'calendar',
  'chats',
  'code',
  'currency',
  'database',
  'envelope',
  'file-pdf',
  'file-text',
  'film',
  'flow',
  'git-branch',
  'globe',
  'graduation',
  'image',
  'package',
  'presentation',
  'search',
  'shield',
  'table',
  'waveform',
] as const

export type SemanticSkillIconKey = (typeof semanticSkillIconKeys)[number]
export type SkillIconOrigin = 'semantic' | 'fallback'

export type SemanticSkillIconSpec = {
  kind: 'semantic'
  key: SemanticSkillIconKey
  tone: ToneName
}

export type ResolvedSkillIconSpec = SemanticSkillIconSpec & { origin: SkillIconOrigin }

const toneNameSet = new Set<string>(toneNames)
const semanticIconKeySet = new Set<string>(semanticSkillIconKeys)

function assertTone(tone: string): asserts tone is ToneName {
  if (!toneNameSet.has(tone)) {
    throw new TypeError(`Unknown Skill icon tone: ${tone}`)
  }
}

function assertSemanticIconKey(key: string): asserts key is SemanticSkillIconKey {
  if (!semanticIconKeySet.has(key)) {
    throw new TypeError(`Unknown semantic Skill icon: ${key}`)
  }
}

export const semanticIcon = (
  key: SemanticSkillIconKey,
  tone: ToneName = 'slate',
): SemanticSkillIconSpec => {
  assertSemanticIconKey(key)
  assertTone(tone)
  return Object.freeze({ kind: 'semantic', key, tone })
}

const genericSemanticRules: ReadonlyArray<{
  pattern: RegExp
  icon: SemanticSkillIconSpec
}> = [
  { pattern: /(?:^|[-_])(security|secure|audit|vet|guard)(?:$|[-_])/, icon: semanticIcon('shield', 'emerald') },
  { pattern: /(?:^|[-_])pdf(?:$|[-_])/, icon: semanticIcon('file-pdf', 'red') },
  { pattern: /(?:^|[-_])(sheet|spreadsheet|excel|xlsx|table)(?:$|[-_])/, icon: semanticIcon('table', 'emerald') },
  { pattern: /(?:^|[-_])(slide|slides|ppt|pptx|presentation)(?:$|[-_])/, icon: semanticIcon('presentation', 'orange') },
  { pattern: /(?:^|[-_])(search|research|discovery)(?:$|[-_])/, icon: semanticIcon('search', 'blue') },
  { pattern: /(?:^|[-_])(browser|web)(?:$|[-_])/, icon: semanticIcon('globe', 'teal') },
  { pattern: /(?:^|[-_])(audio|asr|speech|transcript|voice)(?:$|[-_])/, icon: semanticIcon('waveform', 'cyan') },
  { pattern: /(?:^|[-_])(video|media|editor|caption|captions|subtitle|subtitles)(?:$|[-_])/, icon: semanticIcon('film', 'violet') },
  { pattern: /(?:^|[-_])(image|photo|visual|design)(?:$|[-_])/, icon: semanticIcon('image', 'magenta') },
  { pattern: /(?:^|[-_])(doc|docs|document|paper|article|writing|markdown|note)(?:$|[-_])/, icon: semanticIcon('file-text', 'blue') },
  { pattern: /(?:^|[-_])(mail|email)(?:$|[-_])/, icon: semanticIcon('envelope', 'amber') },
  { pattern: /(?:^|[-_])(calendar|meeting)(?:$|[-_])/, icon: semanticIcon('calendar', 'rose') },
  { pattern: /(?:^|[-_])(database|db|sql|postgres|vault)(?:$|[-_])/, icon: semanticIcon('database', 'indigo') },
  { pattern: /(?:^|[-_])(git|version|publish|publisher|release)(?:$|[-_])/, icon: semanticIcon('git-branch', 'indigo') },
  { pattern: /(?:^|[-_])(skill|plugin|registry|package|installer)(?:$|[-_])/, icon: semanticIcon('package', 'indigo') },
  { pattern: /(?:^|[-_])(code|developer|frontend|backend|app)(?:$|[-_])/, icon: semanticIcon('code', 'violet') },
  { pattern: /(?:^|[-_])(automation|workflow)(?:$|[-_])/, icon: semanticIcon('flow', 'blue') },
  { pattern: /(?:^|[-_])(chat|message|wechat|wecom|lark|slack)(?:$|[-_])/, icon: semanticIcon('chats', 'emerald') },
  { pattern: /(?:^|[-_])(finance|invoice|tax|payment)(?:$|[-_])/, icon: semanticIcon('currency', 'gold') },
  { pattern: /(?:^|[-_])(learn|learning|education|tutorial)(?:$|[-_])/, icon: semanticIcon('graduation', 'blue') },
]

const fallbackIcon = semanticIcon('package', 'slate')

const withOrigin = (
  spec: SemanticSkillIconSpec,
  origin: SkillIconOrigin,
): ResolvedSkillIconSpec =>
  Object.freeze({ ...spec, origin }) as ResolvedSkillIconSpec

export const resolveSkillIconSpec = (rawSkillName: string): ResolvedSkillIconSpec => {
  const skillName = rawSkillName.trim().toLowerCase()

  for (const rule of genericSemanticRules) {
    if (rule.pattern.test(skillName)) return withOrigin(rule.icon, 'semantic')
  }

  return withOrigin(fallbackIcon, 'fallback')
}
