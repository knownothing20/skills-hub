import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { execSync } from 'node:child_process'

// 无论在哪个子目录下运行，始终锚定项目根目录
const __dirname = path.dirname(fileURLToPath(import.meta.url))
const ROOT_DIR = path.resolve(__dirname, '..')

// ANSI 颜色输出
const RED = '\x1b[31m'
const GREEN = '\x1b[32m'
const YELLOW = '\x1b[33m'
const CYAN = '\x1b[36m'
const RESET = '\x1b[0m'
const BOLD = '\x1b[1m'

console.log(`${BOLD}${CYAN}🔍 开始执行发布前隐私与敏感信息安全审计...${RESET}`)
console.log(`根目录: ${ROOT_DIR}\n`)

let hasErrors = false
let warningCount = 0

function error(msg) {
  console.error(`${RED}✖ [错误] ${msg}${RESET}`)
  hasErrors = true
}

function warn(msg) {
  console.warn(`${YELLOW}▲ [警告] ${msg}${RESET}`)
  warningCount++
}

function success(msg) {
  console.log(`${GREEN}✔ [通过] ${msg}${RESET}`)
}

// ----------------------------------------------------
// 1. 检查 Git 追踪文件是否包含黑名单模式
// ----------------------------------------------------
console.log(`${BOLD}[1/4] 检查 Git 追踪文件清单...${RESET}`)
try {
  const trackedFilesOutput = execSync('git ls-files', {
    cwd: ROOT_DIR,
    encoding: 'utf-8',
  })
  const trackedFiles = trackedFilesOutput.split(/\r?\n/).filter(Boolean)

  const FORBIDDEN_FILE_PATTERNS = [
    /^\.env(?!\.example$)/,
    /\.(db|sqlite|sqlite3|db-shm|db-wal)$/i,
    /\.(pem|key|p12|pfx|pkcs12)$/i,
    /^backup\//i,
    /^dist-app\//i,
    /^skills\//i,
    /id_rsa/i,
    /id_ed25519/i,
  ]

  const forbiddenTracked = []
  for (const file of trackedFiles) {
    if (FORBIDDEN_FILE_PATTERNS.some((pattern) => pattern.test(file))) {
      forbiddenTracked.push(file)
    }
  }

  if (forbiddenTracked.length > 0) {
    error(`发现敏感或临时文件被 Git 误追踪:\n  - ${forbiddenTracked.join('\n  - ')}`)
  } else {
    success(`未发现敏感文件被 Git 误追踪（共核验 ${trackedFiles.length} 个被追踪文件）`)
  }
} catch (err) {
  warn(`无法执行 git ls-files，跳过追踪文件排查: ${err.message}`)
}

// ----------------------------------------------------
// 2. 检查 Git 提交者邮箱是否暴露个人真实邮箱
// ----------------------------------------------------
console.log(`\n${BOLD}[2/4] 检查 Git 提交者邮箱配置...${RESET}`)
try {
  const authorsOutput = execSync('git log -n 5 --format="%ae"', {
    cwd: ROOT_DIR,
    encoding: 'utf-8',
  })
  const emails = Array.from(new Set(authorsOutput.split(/\r?\n/).filter(Boolean)))

  if (emails.length > 0) {
    let allProtected = true
    for (const email of emails) {
      if (email.includes('users.noreply.github.com')) {
        success(`最近提交邮箱受 GitHub 隐私保护: ${email}`)
      } else {
        allProtected = false
        warn(`最近提交发现非 noreply 邮箱: ${email} (请确认是否愿意公开此邮箱)`)
      }
    }
  }
} catch (err) {
  warn(`无法获取 Git 提交历史邮箱: ${err.message}`)
}

// ----------------------------------------------------
// 3 & 4. 扫描代码中的敏感凭据、Token 及本地主机路径
// ----------------------------------------------------
console.log(`\n${BOLD}[3/4 & 4/4] 扫描源码与配置中的凭据及本地私密路径...${RESET}`)

const SECRET_PATTERNS = [
  {
    name: 'GitHub Token',
    regex: /(ghp_[a-zA-Z0-9]{36}|github_pat_[a-zA-Z0-9]{82}|gho_[a-zA-Z0-9]{36})/,
  },
  {
    name: 'OpenAI/通用 API Key',
    regex: /(sk-[a-zA-Z0-9]{32,})/,
  },
  {
    name: 'AWS Access Key',
    regex: /(AKIA[0-9A-Z]{16})/,
  },
  {
    name: 'Google API Key',
    regex: /(AIza[0-9A-Za-z-_]{35})/,
  },
  {
    name: '私钥标头',
    regex: /-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----/,
  },
  {
    name: 'Slack/飞书 Webhook',
    regex: /(https:\/\/hooks\.slack\.com\/services\/[A-Za-z0-9\/]+|https:\/\/open\.feishu\.cn\/open-apis\/bot\/v2\/hook\/[a-f0-9-]+)/,
  },
]

// 支持 Windows 正反斜杠匹配的开发机个人路径
const PATH_LEAK_PATTERNS = [
  {
    name: '硬编码 Windows 用户目录',
    regex: /[A-Za-z]:[\\/]Users[\\/](?!example[\\/])[a-zA-Z0-9_-]+/i,
  },
  {
    name: '硬编码 Unix 用户目录',
    regex: /\/Users\/(?!runner|example|may)[a-zA-Z0-9_-]+\/(?!skills)/i,
  },
]

// 忽略扫描的目录与文件
const SCAN_EXCLUDE_PATHS = [
  '.git',
  'node_modules',
  'dist',
  'dist-app',
  'target',
  'backup',
  'skills',
  'scripts/check-privacy.mjs',
]

// 白名单示例占位符
const ALLOWED_MATCHES = [
  'ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx',
  'ghp_your_github_token_here',
]

let totalScannedFiles = 0

function scanDirectory(dir) {
  const entries = fs.readdirSync(dir, { withFileTypes: true })
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name)
    const relPath = path.relative(ROOT_DIR, fullPath).replace(/\\/g, '/')

    if (SCAN_EXCLUDE_PATHS.some((ex) => relPath === ex || relPath.startsWith(`${ex}/`))) {
      continue
    }

    if (entry.isDirectory()) {
      scanDirectory(fullPath)
    } else if (entry.isFile()) {
      if (/\.(ts|tsx|js|mjs|cjs|rs|json|toml|yaml|yml|md|env.*)$/i.test(entry.name)) {
        totalScannedFiles++
        auditFileContent(fullPath, relPath)
      }
    }
  }
}

function auditFileContent(filePath, relPath) {
  const content = fs.readFileSync(filePath, 'utf-8')
  const lines = content.split(/\r?\n/)

  lines.forEach((line, lineIdx) => {
    // 凭据扫描
    for (const rule of SECRET_PATTERNS) {
      const match = line.match(rule.regex)
      if (match) {
        const matchedText = match[0]
        if (!ALLOWED_MATCHES.includes(matchedText)) {
          error(`${relPath}:${lineIdx + 1} 发现疑似 [${rule.name}] 泄露: ${matchedText.slice(0, 8)}...`)
        }
      }
    }

    // 路径泄漏扫描（排查代码和配置，排除 markdown 说明文档中的通用示例）
    if (!relPath.endsWith('.md')) {
      for (const rule of PATH_LEAK_PATTERNS) {
        const match = line.match(rule.regex)
        if (match) {
          warn(`${relPath}:${lineIdx + 1} 发现疑似 [${rule.name}]: ${match[0]}`)
        }
      }
    }
  })
}

try {
  scanDirectory(ROOT_DIR)
  if (!hasErrors) {
    success(`凭据与路径深度扫描完成（共审计 ${totalScannedFiles} 个文本代码/配置文件）`)
  }
} catch (err) {
  error(`代码扫描异常: ${err.message}`)
}

// ----------------------------------------------------
// 审计总结报告
// ----------------------------------------------------
console.log(`\n======================================================`)
if (hasErrors) {
  console.log(`${BOLD}${RED}❌ 隐私与安全审计未通过！请修复上述错误后再行发布。${RESET}`)
  process.exit(1)
} else {
  console.log(`${BOLD}${GREEN}🎉 恭喜！隐私与安全审计全部通过！可以放心发布。${RESET}`)
  if (warningCount > 0) {
    console.log(`${YELLOW}（共有 ${warningCount} 个非阻塞提示，请根据需要核实）${RESET}`)
  }
  process.exit(0)
}
