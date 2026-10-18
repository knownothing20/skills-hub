use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::core::skill_store::SkillStore;
use crate::core::sync_engine::SyncMode;
use crate::core::tool_adapters::{
    default_tool_adapters, is_builtin_tool_enabled, is_tool_installed, load_tool_config,
    project_relative_skills_dir, resolve_default_path, supports_project_scope, CustomToolConfig,
    ToolConfig,
};

pub const RECENT_PROJECTS_SETTING: &str = "recent_projects_v1";
pub const MAX_MANAGED_ICON_DATA_URL_BYTES: usize = 12 * 1024 * 1024;

pub fn format_anyhow_error(err: anyhow::Error) -> String {
    let first = err.to_string();
    // Frontend relies on these prefixes for special flows.
    if first.starts_with("MULTI_SKILLS|")
        || first.starts_with("TARGET_EXISTS|")
        || first.starts_with("TOOL_NOT_INSTALLED|")
        || first.starts_with("TOOL_NOT_WRITABLE|")
    {
        return first;
    }

    // Include the full error chain (causes), not just the top context.
    let mut full = format!("{:#}", err);

    // Redact noisy temp paths from clone context (we care about the cause, not the dest).
    // Example: `clone https://... into "/Users/.../skills-hub-git-<uuid>"`
    if let Some(head) = full.lines().next() {
        if head.starts_with("clone ") {
            if let Some(pos) = head.find(" into ") {
                let head_redacted = format!("{} (已省略临时目录)", &head[..pos]);
                let rest: String = full.lines().skip(1).collect::<Vec<_>>().join("\n");
                full = if rest.is_empty() {
                    head_redacted
                } else {
                    format!("{}\n{}", head_redacted, rest)
                };
            }
        }
    }

    let root = err.root_cause().to_string();
    let lower = full.to_lowercase();

    // Heuristic-friendly messaging for GitHub clone failures.
    if lower.contains("github.com")
        && (lower.contains("clone ") || lower.contains("remote") || lower.contains("fetch"))
    {
        if lower.contains("securetransport") {
            return format!(
                "无法从 GitHub 拉取仓库：TLS/证书校验失败（macOS SecureTransport）。\n\n建议：\n- 检查网络/代理是否拦截 HTTPS\n- 如在公司网络，可能需要安装公司根证书或使用可信代理\n- 也可在终端确认 `git clone {}` 是否可用\n\n详细：{}",
                "https://github.com/<owner>/<repo>",
                root
            );
        }
        let hint = if lower.contains("authentication")
            || lower.contains("permission denied")
            || lower.contains("credentials")
        {
            "无法访问该仓库：可能是私有仓库/权限不足/需要鉴权。"
        } else if lower.contains("not found") {
            "仓库不存在或无权限访问（GitHub 返回 not found）。"
        } else if lower.contains("failed to resolve")
            || lower.contains("could not resolve")
            || lower.contains("dns")
        {
            "无法解析 GitHub 域名（DNS）。请检查网络/代理。"
        } else if lower.contains("timed out") || lower.contains("timeout") {
            "连接 GitHub 超时。请检查网络/代理。"
        } else if lower.contains("connection refused") || lower.contains("connection reset") {
            "连接 GitHub 失败（连接被拒绝/重置）。请检查网络/代理。"
        } else {
            "无法从 GitHub 拉取仓库。请检查网络/代理，或稍后重试。"
        };

        return format!("{}\n\n详细：{}", hint, root);
    }

    full
}

pub fn expand_home_path(input: &str) -> Result<std::path::PathBuf, anyhow::Error> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        anyhow::bail!("storage path is empty");
    }
    if trimmed == "~" {
        let home = dirs::home_dir().context("failed to resolve home directory")?;
        return Ok(home);
    }
    if let Some(stripped) = trimmed.strip_prefix("~/") {
        let home = dirs::home_dir().context("failed to resolve home directory")?;
        return Ok(home.join(stripped));
    }
    Ok(std::path::PathBuf::from(trimmed))
}

pub fn normalize_scope(scope: Option<&str>) -> Result<&'static str, anyhow::Error> {
    match scope.unwrap_or("global") {
        "global" => Ok("global"),
        "project" => Ok("project"),
        other => anyhow::bail!("invalid scope: {}", other),
    }
}

pub fn now_ms() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    now.as_millis() as i64
}

#[derive(Debug, Serialize)]
pub struct ToolInfoDto {
    pub key: String,
    pub label: String,
    pub avatar: Option<String>,
    pub installed: bool,
    pub enabled: bool,
    pub is_custom: bool,
    pub skills_dir: String,
    pub project_skills_dir: String,
    pub supports_project_scope: bool,
    pub sync_mode: SyncMode,
}

#[derive(Debug, Serialize)]
pub struct ToolStatusDto {
    pub tools: Vec<ToolInfoDto>,
    pub installed: Vec<String>,
    pub newly_installed: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolConfigDto {
    pub disabled_builtin_tools: Vec<String>,
    pub custom_tools: Vec<CustomToolConfigDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomToolConfigDto {
    pub key: String,
    pub label: String,
    pub avatar: Option<String>,
    pub skills_dir: String,
    pub project_skills_dir: Option<String>,
    pub sync_mode: SyncMode,
    pub enabled: bool,
}

impl From<ToolConfig> for ToolConfigDto {
    fn from(config: ToolConfig) -> Self {
        Self {
            disabled_builtin_tools: config.disabled_builtin_tools,
            custom_tools: config
                .custom_tools
                .into_iter()
                .map(|tool| CustomToolConfigDto {
                    key: tool.key,
                    label: tool.label,
                    avatar: tool.avatar,
                    skills_dir: tool.skills_dir,
                    project_skills_dir: tool.project_skills_dir,
                    sync_mode: tool.sync_mode,
                    enabled: tool.enabled,
                })
                .collect(),
        }
    }
}

impl From<ToolConfigDto> for ToolConfig {
    fn from(config: ToolConfigDto) -> Self {
        Self {
            disabled_builtin_tools: config.disabled_builtin_tools,
            custom_tools: config
                .custom_tools
                .into_iter()
                .map(|tool| CustomToolConfig {
                    key: tool.key,
                    label: tool.label,
                    avatar: tool.avatar,
                    skills_dir: tool.skills_dir,
                    project_skills_dir: tool.project_skills_dir,
                    sync_mode: tool.sync_mode,
                    enabled: tool.enabled,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeTool {
    pub key: String,
    pub label: String,
    pub avatar: Option<String>,
    pub installed: bool,
    pub enabled: bool,
    pub is_custom: bool,
    pub skills_dir: std::path::PathBuf,
    pub project_skills_dir: String,
    pub supports_project_scope: bool,
    pub sync_mode: SyncMode,
}

pub fn runtime_tools(store: &SkillStore, include_disabled: bool) -> anyhow::Result<Vec<RuntimeTool>> {
    let config = load_tool_config(store)?;
    let mut tools = Vec::new();

    for adapter in default_tool_adapters() {
        let enabled = is_builtin_tool_enabled(&config, adapter.id.as_key());
        if !include_disabled && !enabled {
            continue;
        }
        let detected = is_tool_installed(&adapter)?;
        tools.push(RuntimeTool {
            key: adapter.id.as_key().to_string(),
            label: adapter.display_name.to_string(),
            avatar: None,
            installed: enabled && detected,
            enabled,
            is_custom: false,
            skills_dir: resolve_default_path(&adapter)?,
            project_skills_dir: project_relative_skills_dir(&adapter).to_string(),
            supports_project_scope: supports_project_scope(&adapter),
            sync_mode: SyncMode::Auto,
        });
    }

    for custom in config.custom_tools {
        if !include_disabled && !custom.enabled {
            continue;
        }
        let skills_dir = expand_home_path(&custom.skills_dir)?;
        let supports_project_scope = custom.project_skills_dir.is_some();
        let detected = skills_dir.is_dir();
        tools.push(RuntimeTool {
            key: custom.key,
            label: custom.label,
            avatar: custom.avatar,
            installed: custom.enabled && detected,
            enabled: custom.enabled,
            is_custom: true,
            skills_dir,
            project_skills_dir: custom.project_skills_dir.unwrap_or_default(),
            supports_project_scope,
            sync_mode: custom.sync_mode,
        });
    }

    Ok(tools)
}

pub fn runtime_tool_by_key(store: &SkillStore, key: &str) -> anyhow::Result<RuntimeTool> {
    runtime_tools(store, false)?
        .into_iter()
        .find(|tool| tool.key == key)
        .ok_or_else(|| anyhow::anyhow!("TOOL_NOT_INSTALLED|{}", key))
}

pub fn runtime_tools_sharing_dir(
    store: &SkillStore,
    selected: &RuntimeTool,
    scope: &str,
) -> anyhow::Result<Vec<RuntimeTool>> {
    let tools = runtime_tools(store, false)?;
    let shared = tools
        .into_iter()
        .filter(|tool| {
            tool.installed
                && if scope == "project" {
                    tool.project_skills_dir == selected.project_skills_dir
                } else {
                    tool.skills_dir == selected.skills_dir
                }
        })
        .collect::<Vec<_>>();
    Ok(shared)
}

pub fn resolve_runtime_tool_root(
    tool: &RuntimeTool,
    project_root: Option<&std::path::Path>,
) -> anyhow::Result<std::path::PathBuf> {
    if let Some(project_root) = project_root {
        if !tool.supports_project_scope {
            anyhow::bail!("PROJECT_SCOPE_UNSUPPORTED|{}", tool.key);
        }
        return Ok(project_root.join(&tool.project_skills_dir));
    }
    Ok(tool.skills_dir.clone())
}
