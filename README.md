# Skills Hub (Tauri Desktop)

A cross-platform desktop app (Tauri + React) for installing, organizing, updating, and syncing Agent Skills to multiple AI coding tools' global or project-level skills directories. Skills Hub prefers symlink/junction and automatically falls back to copy when needed: "Install once, sync everywhere".

## Documentation

- English (default): `README.md` (this file)
- 中文说明：[`docs/README.zh.md`](docs/README.zh.md)

---

## 🚀 Recent Major Updates & Enhancements (本次重大升级明细)

> 💡 **Special Note**: This fork incorporates a series of production-grade architectural and visual upgrades over upstream, solving real-world multi-tool synchronization pain points, startup lag, and complex version drift.

### 1. 🔄 Multi-Tool Version Comparison & Reverse Promotion (多端版本比对与反向设为母版)
- **Pain Point**: Previously, synchronization was strictly one-way (Master ➡️ Tools). If a developer edited or improved code directly inside a tool (e.g. OpenCode), pushing an update would overwrite their local work with stale master code.
- **Solution**:
  - Real-time comparison of file modification timestamps (`mtime`) and file trees between each downstream tool and the central master (`~/.agents/skills`).
  - **⬆️ Promote to Master (设为母版)**: Safely push tool-specific modifications back to the global master library with automatic `.skillignore` filtering (excluding local keys, cache, and private logs).
  - **⬇️ Update Tool (更新此软件)**: 1-click push of the latest master updates downstream.

---

### 2. 🎨 Intelligent Three-State Tool Model & Clean UI (工具三态智能模型与极简视觉)
- **Pain Point**: Upstream blindly assumed all tools require duplicate file copies, showing 13+ cramped icons where uninstalled tools cluttered the screen and natively compatible tools appeared dead (gray `0/13`).
- **Solution**:
  - 🎨 **Natively Recognized (原生直接可用)**: Tools supporting universal agent specs (e.g. **Codex**, **Antigravity**) show **pure vibrant brand logos without borders or dots**—ready to run out of the box with zero extra disk footprint!
  - 🟢 **Dedicated Installed (专属物理安装)**: Tools with physical directory copies (e.g. **OpenCode**, **WorkBuddy**, **Trae**) show a **clear green status ring + green badge**.
  - ⚪ **Uninstalled (未同步)**: Displayed in subtle monochrome gray—**click any gray icon to sync in 1 second!**
  - **Eliminated Ghost Tools**: Removed 40+ non-existent tools from clogging the UI.
  - **Breathable 8px Spacing**: Expanded icon size to **28px** with standard `gap: 8px`, eliminating overlapping or misclicks.

---

### 3. ⚡ Blazing Fast Startup & Anti-Freeze Engine (极速秒开与防卡顿引擎)
- **Pain Point**: When managing thousands of project files, launching the app blocked the main thread for seconds, causing visible UI freeze and sluggishness.
- **Solution**:
  - Fully offloaded startup adoption (`adopt_existing_central_skills`) and description backfilling to background `spawn_blocking` workers (0ms main thread release).
  - Onboarding scanner bypasses redundant SHA256 hashing for existing master skills, saving 5,000+ unnecessary filesystem traversals.
  - Front-end onboarding deferred to avoid first-render resource contention.

---

### 4. 🛡️ Master Library Isolation & Safe Unsync (中心母版库隔离与安全守护)
- **Pain Point**: Upstream Cline adapter hardcoded `.agents/skills` as its own target, inadvertently hijacking the central master and crashing with `UNSAFE_PATH` upon unsync.
- **Solution**:
  - Corrected Cline global directory to isolated `.cline/skills`.
  - Added strict protection to `unsync_skill_from_tool`: when a target path coincides with the master root, only deregister the database entry—never touch or delete master files.

---

## Fork Lineage

This repository is an MIT-licensed fork of [qufei1993/skills-hub](https://github.com/qufei1993/skills-hub). It continues the upstream `v0.9.1` code line, includes the current upstream featured-Skill data, and keeps the upstream attribution while adding a fixed `~/.agents/skills` source of truth, hardened local file operations, Trash-only deletion, clearer automatic-update eligibility, and Skill-owned icon metadata.

This fork currently publishes source code only. It does not publish signed installers, updater artifacts, packages, or an automatic binary release.

## Why Skills Hub

AI coding tools increasingly use their own skills directories and installation flows. Maintaining those directories manually can quickly become messy: the same skill gets copied many times, update sources become unclear, tool activation states drift, and bulk cleanup takes too much effort.

Skills Hub installs skills into one central repository, then syncs them to tools such as Claude Code, Codex, Cursor, OpenCode, and Antigravity based on your choices. You can tag skills, choose global or project scope, update tool targets in bulk, and let the system update Git and independent local-source skills on a schedule.

## Key Features

- **Centralized library**: Install skills into one central repository instead of scattering copies across tool folders.
- **Multi-Tool Comparison & Reverse Promotion**: Compare mtime timestamps and file counts between downstream tools and central master. Promote modifications from any tool back to the central repository with `.skillignore` safety filtering.
- **Three-State Tool Model**:
  - 🎨 **Natively Recognized (Directly Usable)**: Tools like Codex and Antigravity read the shared pool natively without duplicate copies, saving disk space.
  - 🟢 **Dedicated Installed**: Tools with physical copies in their private folders (e.g., OpenCode, WorkBuddy) feature a clear status ring and green indicator.
  - ⚪ **Uninstalled**: Shown cleanly in subtle gray, ready for 1-click sync.
- **Blazing Fast Startup**: Asynchronous background startup and smart bypass of redundant folder hashing for instant zero-lag launch.
- **Explore and install**: Install from curated lists, online search, local folders, or Git repositories.
- **Multi-tool sync**: Sync skills to different AI coding tools by global or project scope.
- **Bulk management**: Safely manage skills, batch apply tags, configure tool targets, toggle active states, or perform Trash-only deletions.
- **Tag organization**: Filter, group, and maintain skills with tags.
- **Tool management**: Enable built-in tool targets or add custom skills directories.
- **Detail view**: Browse skill file trees, Markdown content, and code snippets.
- **Migration**: Scan and import existing local skills into one managed library.
- **Discovery controls**: Choose which installed tool directories participate in import discovery.

## Interface Preview

### My Skills — Managed Skills and Bulk Actions

My Skills provides card and list views for each managed skill's source, tags, sync scope, target tools, and enabled state. The toolbar supports scope filtering, sorting, tag filtering, search, and bulk actions.

When Skills Hub discovers importable Skills in installed tool directories, the discovery banner lets you review them or open Scan settings. Scan sources are independent from sync targets, persist across restarts, and remain accessible from Settings. Only directories containing `SKILL.md` are shown as importable Skills.

![My Skills card view](docs/assets/my-skills-card-view.png)

![My Skills list view with bulk actions](docs/assets/my-skills-list-bulk-actions.png)

### Explore — Curated Skills and Online Search

Explore brings together curated repository skills and online search. After clicking Install, you can choose tags, install scope, and target tools.

![Explore online skills](docs/assets/explore-online-skills.png)

### Add Skill — Set Tags, Scope, and Tools Before Installation

Manual add supports both local folders and Git repositories. Before installing, you can assign tags, choose global or project scope, and choose which tools to sync to.

![Add a skill from a Git repository](docs/assets/add-skill-git-repository.png)

### Tools — Built-in and Custom Tool Management

Tools shows detected and enabled AI coding tools with recognizable product icons. You can enable built-in targets or create and edit custom tools with an avatar, skills directories, and an explicit sync mode.

![Built-in and custom tool management](docs/assets/tools-management.png)



### Settings — App-Level Preferences

Settings keeps local app preferences such as interface language, appearance, discovery scanning, Git cache, and the restricted loopback network proxy. This fork intentionally fixes the central library location and does not enable the upstream in-app updater.

![Application preferences](docs/assets/settings-app-preferences.png)

## Workflow (Bi-Directional Closed-Loop Lifecycle)

1. **Install and Manage**: Install a skill from Explore, a local folder, or Git repository into the fixed central source-of-truth `~/.agents/skills`.
2. **Native Recognition & Sync**:
   - Tools adopting the universal agent specification (e.g. Codex, Antigravity) **natively use the master library without duplicate copies**.
   - Tools using isolated private environments (e.g. OpenCode, WorkBuddy) can receive dedicated installations with 1-click sync.
3. **Multi-Tool Comparison & Reverse Promotion**:
   - If code changes or updates are made inside a downstream tool, open the skill's **Multi-Tool Comparison** modal. The system detects version drift based on mtime and file trees.
   - Click **⬆️ Promote to Master** to safely promote the tool's latest modifications back to `~/.agents/skills` (safeguarded by `.skillignore`).
   - Click **⬇️ Update Tool** to push master changes downstream, achieving full bi-directional consistency!
4. **Maintenance**: Organize with tags, enable/disable, or move to Trash at any time.

## 🌟 Intelligent Three-State Tool Model

Skill cards feature an intuitive three-state interaction model rather than simplistic on/off toggles:

| State | Visual Appearance | Meaning & Interaction | Examples |
| :--- | :--- | :--- | :--- |
| **Natively Recognized** | **Pure vibrant brand logo** (no borders or dots) | Tool reads the central master directory natively. Usable out of the box with zero extra disk footprint. | Codex, Antigravity |
| **Dedicated Installed** | **Color logo + Green status ring + Green dot** | Tool has an independent physical installation inside its private directory. | OpenCode, WorkBuddy, Trae |
| **Uninstalled** | **Subtle monochrome gray** (no borders or dots) | Skill is not yet synced to this tool. **Click the gray icon to sync in 1 second!** | Any installed AI tool on machine |

> **Clean Layout**: Uninstalled ghost tools are hidden from cards to save space, and icons feature a breathable 8px gap to prevent overlap or misclicks.

## Skill-Provided Icons

Skills Hub does not guess a Skill's publisher or maintain a built-in mapping from Skill names to personal avatars. A Skill can own its icon through the standard Codex UI metadata file `agents/openai.yaml`, with image files stored inside that Skill:

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

The managed-Skills list prefers `icon_small`, falls back to `icon_large`, and then uses a generic semantic icon. This keeps icon customization with the Skill, so authors and users can replace an icon without changing Skills Hub source code.

The card renders one icon edge-to-edge in its 48 px rounded tile with no overlay badge. For consistent optical fill, use a square asset with a tightly cropped artboard; transparent padding inside the source image remains part of the image and should be removed from the asset itself.

For safety, icon paths must be relative, stay inside the Skill directory after canonicalization, and point to a regular non-symlink SVG, PNG, JPEG, or WebP file no larger than 128 KiB. Raster icons are limited to 512×512 and 262,144 pixels. URLs, absolute paths, `..`, active SVG content, oversized raster dimensions, mismatched file signatures, and invalid colors are ignored. The parser reads the block-style `interface` keys shown above. `brand_color` is optional and must use `#RRGGBB`. The managed list also caps the combined encoded icon payload at 12 MiB; icons beyond that response budget fall back to the generic icon without changing Skill health.

## Supported Core AI Coding Tools

Skills Hub provides deep compatibility and safety hardening for major AI coding tools. Tools adhering to universal agent specs read the master library natively with zero duplication, while isolated environments receive dedicated physical installations with 1-click sync:

| Tool Key | Name | Global Skills Dir (relative to `~`) | Integration & Sync Mode |
| :--- | :--- | :--- | :--- |
| `codex` | **Codex** | `.agents/skills` (native shared pool) / `.codex/skills` | 🎨 Natively recognized, zero extra disk footprint; dedicated copy optional |
| `antigravity` | **Antigravity** | `.agents/skills` / `.gemini/config/skills` | 🎨 Natively recognized, zero extra disk footprint |
| `opencode` | **OpenCode** | `.config/opencode/skills` | 🟢 Dedicated installation (supports 1-click sync & reverse promotion) |
| `workbuddy` | **WorkBuddy** | `.workbuddy/skills` | 🟢 Dedicated installation (supports 1-click sync & reverse promotion) |
| `trae_cn` | **Trae CN** | `.trae-cn/skills` | 🟢 Dedicated physical installation |
| `openclaw` | **OpenClaw** | `.openclaw/skills` | 🟢 Dedicated physical installation |
| `claude_code` | **Claude Code** | `.claude/skills` | 🟢 Dedicated physical installation |
| `cline` | **Cline** | `.cline/skills` | 🟢 Dedicated installation (corrected to dedicated path, resolving master conflicts) |
| `cursor` | **Cursor** | `.cursor/skills` | 🟢 Dedicated physical installation |
| `windsurf` | **Windsurf** | `.codeium/windsurf/skills` | 🟢 Dedicated physical installation |

> See [`src-tauri/src/core/tool_adapters/mod.rs`](src-tauri/src/core/tool_adapters/mod.rs) for complete path rules and adapter definitions.

## Development

### Prerequisites

- Node.js 18+ (recommended: 20+)
- Rust (stable)
- Tauri system dependencies (follow Tauri official docs for your OS)

```bash
npm install
npm run tauri:dev
```

### Build

```bash
npm run lint
npm run build
npm run tauri:build
```

#### Platform build commands (from `package.json`)

- macOS (dmg): `npm run tauri:build:mac:dmg`
- macOS (universal dmg): `npm run tauri:build:mac:universal:dmg`
- Windows (MSI): `npm run tauri:build:win:msi`
- Windows (NSIS exe): `npm run tauri:build:win:exe`
- Windows (MSI+NSIS): `npm run tauri:build:win:all`
- Linux (deb): `npm run tauri:build:linux:deb`
- Linux (AppImage): `npm run tauri:build:linux:appimage`
- Linux (deb+AppImage): `npm run tauri:build:linux:all`

### Tests (Rust)

```bash
cd src-tauri
cargo test
```

## Contributing & Security

- Contributing: [`CONTRIBUTING.md`](CONTRIBUTING.md)
- Code of Conduct: [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md)
- Security: [`SECURITY.md`](SECURITY.md)

## FAQ / Notes

- Where are skills stored? This fork fixes the central source of truth at `~/.agents/skills`; tool-specific directories are validated sync targets rather than co-equal sources.
- What are tags for? Tags help you find and organize skills. They do not change where a skill is synced or which tools can use it.
- What is Management Center for? Management Center handles tags, tool targets, and automatic skill updates. Settings keeps app-level preferences.
- Does disabling a skill delete files? No. Disabling only removes tool-side sync. The skill and its configuration remain in the Central Repo and can be enabled again later.
- What does bulk tool setup mean? Skills Hub applies the currently selected tool list to the selected skills. Unchecked tools are removed from those skills' sync targets.
- What is project-level sync? The skill is still stored once in the Central Repo, but its sync target is a selected project directory such as `<project>/.agents/skills`, `<project>/.claude/skills`, or another tool-specific project skills path.
- What is a custom tool directory? If an internal tool or wrapped agent has its own skills directory, you can add it in Management Center as a custom sync target.
- What does automatic update update? It updates Git skills and local skills with an independent external source, then refreshes validated copy-mode targets. Managed skills that only point back to `~/.agents/skills` are not self-updated.
- Which requests use the network proxy? It affects GitHub API calls, curated skill lists, GitHub Contents downloads, and Git clone/fetch/update flows.
- Why is Cursor sync always copy? Cursor currently does not support symlink/junction-based skill directories, so Skills Hub forces directory copy when syncing to Cursor.
- Why does sync sometimes fall back to copy? Skills Hub prefers symlink/junction, but on some systems (especially Windows) symlinks may be restricted; in that case it falls back to directory copy.
- What does `TARGET_EXISTS|...` mean? The target folder already exists and the operation did not overwrite it (default is non-destructive). Remove the existing folder or retry with the appropriate overwrite flow.
- macOS Gatekeeper note (unsigned/notarized builds, may vary by macOS version): if you see “damaged” or “unverified developer”, run `xattr -cr "/Applications/Skills Hub.app"` (https://v2.tauri.app/distribute/#macos).

## Supported Platforms

- macOS (verified)
- Windows (expected by design; not validated locally)
- Linux (expected by design; not validated locally)

## License

MIT License — see `LICENSE`.
