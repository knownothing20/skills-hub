# Skills Hub（Tauri Desktop）

一个跨平台桌面应用（Tauri + React），用于集中安装、整理、更新 Agent Skills，并把它们同步到多个 AI 编程工具的全局或项目级 skills 目录。Skills Hub 会优先使用 symlink/junction，同步失败时自动回退到 copy，实现 “Install once, sync everywhere”。

> English documentation: [`README.md`](../README.md)

---

## 🚀 本次重大升级与深度优化明细

> 💡 **特别说明**：针对上游版本在真实高频多工具联动场景下的核心痛点（如单向覆盖丢失代码、启动卡顿、未安装软件占位拥挤等），本次更新完成了整套生产级架构重构与视觉模型升级：

### 1. 🔄 多端版本比对与反向设为母版（打破单向限制，形成双向闭环）
- **核心痛点**：过去仅支持“母版 ➡️ 工具”单向覆盖。若开发者在具体 AI 工具（如 OpenCode）中调试优化了技能代码，一旦点击同步，工具里的最新修改就会被旧母版无情覆盖！
- **全新能力**：
  - **毫秒级时间戳比对**：精准提取母版与各工具目录的最新文件修改时间（`mtime`）和文件总数，智能标注【工具较新】、【母版较新】或【完全一致】；
  - **⬆️ 设为母版（Promote to Master）**：可一键将该工具中的最新成果安全反向提拔为全局母版，并自动加载 `.skillignore` 排除私密 Token、日志和缓存文件；
  - **⬇️ 更新此软件**：用母版最新代码一键更新指定下游工具；
  - **界面重构**：将操作按钮提至宽阔通栏大标题行，彻底消灭挤压换行与标题竖排 Bug。

---

### 2. 🎨 智能三态工具模型与清爽极简视觉（告别死板 0/13）
- **核心痛点**：上游一刀切假设所有工具都需要物理拷贝，不仅导致支持全局规范的 Codex/Antigravity 被死板显示为灰色 `0/13`，而且把电脑上根本没装的数十个生僻工具全堆在卡片上，排成长龙极其拥挤。
- **全新能力**：
  - 🎨 **原生可识别 (可直接使用)**：像 **Codex**、**Antigravity** 这类原生直接读取全局池的工具，展示**纯净原彩色品牌 Logo（无圈无小点）**，天生可用且零硬盘冗余；
  - 🟢 **专属物理安装**：已在私有专属目录建立独立文件副本的工具（如 **OpenCode**、**WorkBuddy**、**Trae**），标示**精致绿色外圈 + 右上角实心绿微标**；
  - ⚪ **未安装 (待同步)**：低调纯灰度呈现，**点击灰色图标即可一秒一键同步安装**；
  - **彻底清除幽灵工具**：电脑上完全没有安装的生僻工具彻底不显示，杜绝无效占地；
  - **舒展 8px 间距**：取消负边距遮挡，采用 `gap: 8px` 与 28px 饱满尺寸，点击手感极佳！

---

### 3. ⚡ 极速秒开与防卡死底层优化（从几秒卡顿到瞬间拉起）
- **核心痛点**：中心库纳管上千个项目文件时，旧版在启动主线程中执行深层目录同步和递归 SHA256 哈希计算，导致界面启动明显卡顿转圈。
- **全新能力**：
  - 将启动登记与描述回填彻底异步化移至后台 `spawn_blocking`，主线程 0ms 瞬间释放；
  - 在扫描最底层直接跳过母版已知技能，彻底规避 5,000+ 个文件的重复递归遍历与昂贵哈希运算；
  - 前端导入计划延迟 4000ms 加载，首屏零竞争。

---

### 4. 🛡️ 中心母版库绝对安全隔离与解绑修复
- **核心痛点**：旧版 Cline 适配器误将中心母版 `~/.agents/skills` 设为自己的目录，导致霸占母版库，且在取消同步时触发 `UNSAFE_PATH` 报警。
- **全新能力**：
  - 修正 Cline 专属目录为独立的 `~/.cline/skills`，解除对全局母版的占用；
  - 完善 `unsync_skill_from_tool`：检测到目标为中心母版时仅注销数据库关联，坚决不触碰、不删除任何物理母版文件。

---

## Fork 来源

本仓库是 [qufei1993/skills-hub](https://github.com/qufei1993/skills-hub) 的 MIT 许可 fork，沿用上游 `v0.9.1` 代码线、包含当前上游精选 Skill 数据，并保留上游署名。在此基础上增加了固定以 `~/.agents/skills` 为唯一真源、更严格的本地文件安全边界、仅移入废纸篓的删除方式、更清晰的自动更新资格判断，以及由 Skill 自己提供图标元数据的机制。

这个 fork 目前只公开源码，不发布签名安装包、更新产物、软件包或自动生成的二进制 Release。

## 为什么使用 Skills Hub

AI 编程工具越来越多，每个工具都有自己的 skills 目录和安装方式。手动维护这些目录会带来几个问题：同一个 Skill 要复制多份、更新来源不清楚、不同工具启用状态不一致、批量整理成本高。

Skills Hub 的做法是：把 Skill 统一安装到中心仓库，再按你的选择同步到 Claude Code、Codex、Cursor、OpenCode、Antigravity 等工具。你可以为 Skill 打标签、选择全局或项目范围、批量调整工具目标，也可以让系统定时帮你更新 Git 和具有独立外部来源的本地 Skill。

## 主要功能

- **集中托管**：把 Skill 安装到中心仓库，避免分散在多个工具目录里。
- **多端对比与反向设为母版**：实时比对各个 AI 工具与全局母版的文件修改时间戳（mtime）与文件树。在具体工具中修改代码后，支持一键反向提拔为全局母版（遵循 `.skillignore` 排除私密文件），告别旧母版误覆盖！
- **工具三态智能模型**：
  - 🎨 **原生可识别（直接可用）**：Codex、Antigravity 原生读取全局池，呈现纯彩色品牌 Logo，无需多余拷贝、省硬盘空间；
  - 🟢 **专属物理安装**：已在 OpenCode、WorkBuddy 私有目录安装独立副本的工具，带专属绿色外圈与实心徽标；
  - ⚪ **未安装**：纯灰色低调呈现，点击即可一键分发安装。
- **极速启动与防卡死**：全异步化后台启动与深度优化扫描，跳过海量冗余哈希计算，瞬间秒开零延迟。
- **探索安装**：从精选列表、在线搜索、本地目录或 Git 仓库安装 Skill。
- **多工具同步**：按全局或项目范围同步到不同 AI 编程工具。
- **批量管理**：批量管理 Skill，设置标签、同步工具目标、启用状态，以及仅移入废纸篓的安全删除。
- **标签整理**：用标签筛选、归类和维护 Skill。
- **工具管理**：启用内置工具，也可以添加自定义工具目录。
- **详情查看**：浏览 Skill 文件树、Markdown 内容和代码片段。
- **迁移接管**：扫描并导入本机已有 Skills，统一纳入管理。
- **发现控制**：选择哪些已安装工具目录参与可导入 Skill 扫描。

## 界面预览

### My Skills — 托管技能与批量管理

My Skills 通过卡片和列表两种视图展示已托管 Skill 的来源、标签、同步范围、目标工具和启用状态。顶部可以筛选范围、排序、按标签筛选、搜索或执行批量操作。

Skills Hub 在已安装工具目录中发现可导入 Skill 后，会显示发现提示。用户可以查看并导入，或打开“扫描设置”按实际目录控制扫描来源；该设置与同步目标相互独立、重启后保留，并可随时从设置页重新打开。只有包含 `SKILL.md` 的目录会作为可导入 Skill 展示。

![My Skills 卡片视图](./assets/my-skills-card-view.png)

![My Skills 列表视图与批量操作](./assets/my-skills-list-bulk-actions.png)

### Explore — 精选 Skill 与在线搜索

Explore 汇总精选仓库中的 Skill，并支持在线搜索。点击 Install 后可以继续选择标签、安装范围和目标工具。

![在线探索 Skills](./assets/explore-online-skills.png)

### Add Skill — 安装前设置标签、范围和工具

手动添加支持本地目录和 Git 仓库。安装前可以设置标签，选择全局或项目范围，并选择要同步到哪些工具。

![从 Git 仓库添加 Skill](./assets/add-skill-git-repository.png)

### Tools — 内置与自定义工具管理

工具页集中展示已检测和已启用的 AI 编程工具，并使用对应产品图标增强识别。你可以启用内置目标，也可以为自定义工具配置头像、Skills 目录和明确的同步模式，并在创建后继续编辑。

![内置与自定义工具管理](./assets/tools-management.png)



### Settings — 应用级设置

设置页只保留本地应用偏好：界面语言、外观、发现扫描、Git 缓存和受限的本机回环代理。这个 fork 固定中央目录，并且不启用上游的应用内在线更新。

![应用偏好设置](./assets/settings-app-preferences.png)

## 工作方式（双向闭环生态）

1. **安装与纳管**：从 Explore、本地目录或 Git 仓库安装 Skill，自动统一落盘到中心真源 `~/.agents/skills`。
2. **多端原生识别与同步**：
   - 像 Codex、Antigravity 这类支持全局 Agent 规范的工具，**无需拷贝即可直接原生调用**；
   - 像 OpenCode、WorkBuddy 等独立工具，支持一键分发独立副本到其私有目录。
3. **多端版本比对与反向设为母版**：
   - 如果你在某个工具（如 OpenCode）中调试优化了代码，打开详情页【多端对比】，系统自动通过修改时间戳（mtime）识别出【工具较新】；
   - 点击【⬆️ 设为母版】，即可将最新代码安全同步回 `~/.agents/skills` 官方母版（严格遵循 `.skillignore` 排除私密文件）；
   - 点击【⬇️ 更新此软件】，可用母版最新成果一键更新任意工具，形成真正的双向管理闭环！
4. **日常整理与维护**：在 My Skills 中随时启停、分类整理或移入废纸篓。

## 🌟 工具三态智能视觉模型

卡片上的工具图标不再粗暴地全黑或全亮，而是遵循严谨的三态交互模型：

| 状态类别 | 视觉表现 | 交互与含义 | 对应工具示例 |
| :--- | :--- | :--- | :--- |
| **原生可识别 (可直接使用)** | **纯净品牌原彩色 Logo**（无圈无杂标） | 该工具原生支持直接读取母版池，**天生能用、无需多占硬盘** | Codex, Antigravity |
| **专属物理安装** | **彩色 Logo + 精致状态绿圈 + 右上角实心徽标** | 该工具已在私有专属目录建立独立文件副本供其运行 | OpenCode, WorkBuddy, Trae |
| **未安装 (待同步)** | **纯灰色半透明**（无圈无标） | 该工具暂未安装此技能，**鼠标点击灰色图标即可一秒完成一键同步安装** | 任意本机已安装的 AI 工具 |

> **提示**：系统会自动隐藏本机完全未安装的幽灵软件（不再有长串无意义的灰色占位符），并支持 8px 舒展呼吸感排列，彻底告别错位挤压。

## 由 Skill 自己提供图标

Skills Hub 不会猜测 Skill 的发布者，也不在应用源码里维护“Skill 名称 → 个人头像”的硬编码表。每个 Skill 可以通过 Codex 标准 UI 元数据 `agents/openai.yaml` 自带图标，图片放在该 Skill 自己的目录中：

```text
my-skill/
├── SKILL.md
├── agents/
│   └── openai.yaml
└── assets/
    ├── icon-small.svg
    └── logo-large.png
```

```yaml
interface:
  icon_small: "./assets/icon-small.svg"
  icon_large: "./assets/logo-large.png"
  brand_color: "#3B82F6"
```

Skills 列表优先显示 `icon_small`，无效或缺失时尝试 `icon_large`，再没有则回退到通用语义图标。这样图标跟随 Skill 自身；作者或使用者替换自己 Skill 的图标时，不需要修改 Skills Hub 源码。

卡片会把唯一一张主图标铺满 48 px 圆角格，不叠加角标。为了让视觉主体像 Obsidian 图标一样饱满，建议使用画布裁切紧凑的正方形素材；源图片自身的透明留白仍属于图片内容，需要在素材文件里先裁掉。

安全限制：图标必须使用相对路径，规范化后仍位于该 Skill 目录内，并且是小于等于 128 KiB 的普通非符号链接 SVG、PNG、JPEG 或 WebP 文件；栅格图不得超过 512×512 和 262,144 像素。URL、绝对路径、`..`、主动 SVG 内容、超大像素尺寸、扩展名与文件内容不一致的图片以及非法颜色都会被忽略。解析器读取上例所示的块状 `interface` 字段。`brand_color` 可选，格式必须是 `#RRGGBB`。Skills 列表的一次响应还会把全部图标的编码后总量限制在 12 MiB；超过预算的后续图标只回退到通用图标，不影响 Skill 健康状态。

## 支持的核心主流 AI 编程工具

Skills Hub 针对主流 AI 编程工具进行了深度适配与安全加固。对于支持全局 Agent 规范的工具可原生零冗余直接读取母版；对于独立目录工具支持一键建立物理安装副本：

| 工具 Key | 软件名称 | 全局 Skills 目录（相对 `~`） | 生效与适配模式 |
| :--- | :--- | :--- | :--- |
| `codex` | **Codex** | `.agents/skills`（原生母版池）/ `.codex/skills` | 🎨 原生直接识别，无需二次拷贝；支持专属安装 |
| `antigravity` | **Antigravity** | `.agents/skills` / `.gemini/config/skills` | 🎨 原生直接识别，无需二次拷贝 |
| `opencode` | **OpenCode** | `.config/opencode/skills` | 🟢 专属物理安装（支持一键更新与反向提拔为母版） |
| `workbuddy` | **WorkBuddy** | `.workbuddy/skills` | 🟢 专属物理安装（支持一键更新与反向提拔为母版） |
| `trae_cn` | **Trae CN** | `.trae-cn/skills` | 🟢 专属物理安装 |
| `openclaw` | **OpenClaw** | `.openclaw/skills` | 🟢 专属物理安装 |
| `claude_code` | **Claude Code** | `.claude/skills` | 🟢 专属物理安装 |
| `cline` | **Cline** | `.cline/skills` | 🟢 专属物理安装（已修正独立目录，解除母版冲突） |
| `cursor` | **Cursor** | `.cursor/skills` | 🟢 专属物理安装 |
| `windsurf` | **Windsurf** | `.codeium/windsurf/skills` | 🟢 专属物理安装 |

> 完整工具定义与自定义工具扩展规范见 [`src-tauri/src/core/tool_adapters/mod.rs`](../src-tauri/src/core/tool_adapters/mod.rs)。

## 开发

### 环境要求

- Node.js 18+（建议 20+）
- Rust（stable）
- Tauri 系统依赖（按官方文档安装）

### 启动（桌面端）

```bash
npm install
npm run tauri:dev
```

### 构建

```bash
npm run lint
npm run build
npm run tauri:build
```

#### 各系统构建命令（来自 `package.json`）

- macOS（dmg）：`npm run tauri:build:mac:dmg`
- macOS（universal dmg）：`npm run tauri:build:mac:universal:dmg`
- Windows（MSI）：`npm run tauri:build:win:msi`
- Windows（NSIS exe）：`npm run tauri:build:win:exe`
- Windows（MSI+NSIS）：`npm run tauri:build:win:all`
- Linux（deb）：`npm run tauri:build:linux:deb`
- Linux（AppImage）：`npm run tauri:build:linux:appimage`
- Linux（deb+AppImage）：`npm run tauri:build:linux:all`

### 测试（Rust）

```bash
cd src-tauri
cargo test
```

## FAQ / 备注

- Skill 存在哪里？这个 fork 把 `~/.agents/skills` 固定为唯一真源；各工具目录只是经过校验的同步目标。
- 标签用于什么？标签只用于查找和整理 Skill，不会改变 Skill 的同步目录，也不会改变哪些工具可以使用它。
- 管理中心用于什么？管理中心负责标签、工具目标和 Skills 自动更新；设置页只保留应用级配置。
- 停用 Skill 会删除文件吗？不会。停用只会移除工具侧同步，中心仓库中的 Skill 和配置仍保留，重新启用后可按原工具设置恢复。
- 批量设置工具是什么意思？对选中的 Skill 应用当前勾选的工具列表；未勾选的工具会从这些 Skill 的同步目标中移除。
- 什么是项目级同步？Skill 仍然只在中心仓库保存一份，但同步目标变为指定项目目录，例如 `<project>/.agents/skills`、`<project>/.claude/skills` 或其它工具对应的项目级 skills 路径。
- 自定义工具目录是什么？如果某个内部工具或二次封装 Agent 使用自己的 skills 目录，可以在管理中心添加为自定义同步目标。
- 自动更新会更新什么？自动更新只处理 Git Skill 和具有独立外部来源的本地 Skill，并刷新经过校验的 copy 模式目标；只指回 `~/.agents/skills` 的受管 Skill 不会自我更新。
- 网络代理影响哪些请求？它会影响 GitHub API、精选 Skills、GitHub Contents 下载和 Git clone/fetch/update 流程。
- Cursor 为什么强制 Copy？Cursor 当前不支持软链（symlink/junction）形式的技能目录，因此同步到 Cursor 时会固定使用目录复制（copy）。
- 为什么有时会变成 Copy？默认优先 symlink/junction，但在某些系统（尤其 Windows）可能因为权限/策略导致无法创建链接，会自动回退到目录复制。
- `TARGET_EXISTS|...` 是什么意思？目标目录已存在且默认不覆盖（为了安全）。你需要先清理目标目录，或在“接管/覆盖”的明确流程里重试。
- macOS Gatekeeper 备注（未签名/未公证构建，不同 macOS 版本表现可能不同）：如提示“已损坏/无法验证开发者”，可执行 `xattr -cr "/Applications/Skills Hub.app"`（https://v2.tauri.app/distribute/#macos）。

## 支持的系统

- macOS（已验证）
- Windows（按架构应支持，未做本地验证）
- Linux（按架构应支持，未做本地验证）

## License

MIT License（见 `LICENSE`）。
