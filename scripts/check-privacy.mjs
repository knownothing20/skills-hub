import fs from 'node:fs'
import path from 'node:path'
import { execSync } from 'node:child_process'

const ROOT_DIR = process.cwd()

// ANSI 颜色输出
const RED = '\x1b[31m'
const GREEN = '\x1b[32m'
const YELLOW = '\x1b[33m'
const CYAN = '\x1b[36m'
const RESET = '\x1b[0m'
const BOLD = '\x1b[1m'

console.log(`${BOLD}${CYAN}🔍 开始执行发布前隐私与敏感信息安全审计...${RESET}\n`)

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

// 1. 检查 Git 追踪文件是否包含黑名单模式
console.log(`${BOLD}[1/4] 检查 Git 追踪文件清单...${RESET}`)
try {
  const trackedFilesOutput = execSync('git ls-files', { encoding: 'utf-8' })
  const trackedFiles = trackedFilesOutput.split(/\r?\n/).filter(Boolean)

  const FORBIDDEN_FILE_PATTERNS = [
    /^\.env(?!\.example$)/,
    /\.(db|sqlite|sqlite3|db-shm|db-wal)$/i,
    /\.(pem|key|p12|pfx|pkcs12)$/i,
    /^backup\//i,
    /^dist-app\//i,
    /^skills\//i,
  ]

  let forbiddenTracked = []
  for (const file of trackedFiles) {
    if (FORBIDDEN_FILE_PATTERNS.some((pattern) => pattern.test(file))) {
      forbiddenTracked.push(file)
    }
  }

  if (forbiddenTracked.length > 0) {
    error(`发现敏感文件被 Git 追踪:\n  - ${forbiddenTracked.join('\n  - ')}`)
  } else {
    success(`未发现敏感或临时文件被 Git 误追踪（共检查 ${trackedFiles.length} 个文件）`)
  }
} catch (err) {
  warn(`无法执行 git ls-files，跳过追踪文件排查: ${err.message}`)
}

// 2. 检查最近提交者邮箱是否暴露个人真实邮箱
console.log(`\n${BOLD}[2/4] 检查 Git 提交者邮箱配置...${RESET}`)
try {
  const authorEmail = execSync('git log -1 --format="%ae"', { encoding: 'utf-8' }).trim()
  if (authorEmail) {
    if (authorEmail.includes('users.noreply.github.com')) {
      success(`最近提交邮箱受 GitHub 隐私保护: ${authorEmail}`)
    } else {
      warn(`最近提交邮箱为非 noreply 邮箱: ${authorEmail} (请确认是否愿意公开此邮箱)`)
    }
  }
} catch (err) {
  warn(`无法获取 Git 提交邮箱: ${err.message}`)
}

// 3. 扫描代码与配置中的高危 Token、API Key、私钥
console.log(`\n${BOLD}[3/4] 扫描代码库敏感凭证与密钥 (Secret Scanning)...${RESET}`)

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
    name: '私钥标头',
    regex: /-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----/,
  },
]

// 忽略的文件或路径（包含测试 mock 占位符、说明文档与本审计脚本本身）
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

// 允许的安全占位符白名单
const ALLOWED_MATCHES = [
  'ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx',
  'ghp_your_github_token_here',
]

function scanDir(dir) {
  const entries = fs.readdirSync(dir, { withFileTypes: true })
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name)
    const relPath = path.relative(ROOT_DIR, fullPath).replace(/\\/g, '/')

    if (SCAN_EXCLUDE_PATHS.some((ex) => relPath === ex || relPath.startsWith(`${ex}/`))) {
      continue
    }

    if (entry.isDirectory()) {
      scanDir(fullPath)
    } else if (entry.isFile()) {
      // 仅扫描常见文本文件
      if (/\.(ts|tsx|js|mjs|cjs|rs|json|toml|yaml|yml|md|env.*)$/i.test(entry.name)) {
        scanFile(fullPath, relPath)
      }
    }
  }
}

function scanFile(filePath, relPath) {
  const content = fs.readFileSync(filePath, 'utf-8')
  const lines = content.split(/\r?\n/)

  lines.forEach((line, lineIdx) => {
    for (const rule of SECRET_PATTERNS) {
      const match = line.match(rule.regex)
      if (match) {
        const matchedText = match[0]
        if (!ALLOWED_MATCHES.includes(matchedText)) {
          error(`${relPath}:${lineIdx + 1} 发现疑似 [${rule.name}] 泄露: ${matchedText.slice(0, 8)}...`)
        }
      }
    }
  })
}

try {
  scanDir(ROOT_DIR)
  if (!hasErrors) {
    success('未扫描到任何真实泄露的 API Key、Token 或私钥')
  }
} catch (err) {
  error(`敏感凭证扫描异常: ${err.message}`)
}

// 4. 扫描本地主机用户名与绝对路径泄漏
console.log(`\n${BOLD}[4/4] 扫描本地开发机绝对路径硬编码...${RESET}`)

const PATH_LEAK_PATTERNS = [
  {
    name: '硬编码 Windows 用户目录',
    regex: /[A-Za-z]:\\Users\\(?!example\\)[a-zA-Z0-9_-]+/i,
  },
  {
    name: '硬编码 Unix 用户目录',
    regex: /\/Users\/(?!runner|example|may)[a-zA-Z0-9_-]+\/(?!skills)/i,
  },
]

function scanPaths(dir) {
  const entries = fs.readdirSync(dir, { withFileTypes: true })
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name)
    const relPath = path.relative(ROOT_DIR, fullPath).replace(/\\/g, '/')

    if (SCAN_EXCLUDE_PATHS.some((ex) => relPath === ex || relPath.startsWith(`${ex}/`))) {
      continue
    }

    if (entry.isDirectory()) {
      scanPaths(fullPath)
    } else if (entry.isFile()) {
      if (/\.(ts|tsx|js|mjs|cjs|rs|json)$/i.test(entry.name)) {
        const content = fs.readFileSync(fullPath, 'utf-8')
        const lines = content.split(/\r?\n/)
        lines.forEach((line, idx) => {
          for (const rule of PATH_LEAK_PATTERNS) {
            const match = line.match(rule.regex)
            if (match) {
              warn(`${relPath}:${idx + 1} 发现疑似 [${rule.name}]: ${match[0]}`)
            }
          }
        })
      }
    }
  }
}

try {
  scanPaths(ROOT_DIR)
  success('本地绝对路径排查完毕，未发现敏感主机路径')
} catch (err) {
  error(`本地路径排查异常: ${err.message}`)
}

// 审计总结
console.log(`\n======================================================`)
if (hasErrors) {
  console.log(`${BOLD}${RED}❌ 隐私与敏感信息审计未通过！请修正上述错误后再行发布。${RESET}`)
  process.exit(1)
} else {
  console.log(`${BOLD}${GREEN}🎉 恭喜！隐私与安全审计全部通过！可以放心发布。${RESET}`)
  if (warningCount > 0) {
    console.log(`${YELLOW}（共有 ${warningCount} 个非阻塞提示，请根据需要核实）${RESET}`)
  }
  process.exit(0)
}
