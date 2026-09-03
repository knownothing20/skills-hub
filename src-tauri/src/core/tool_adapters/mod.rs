use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::safe_fs::{
    direct_skill_child, ensure_distinct_roots, lock_central_mutation, move_internal_to_trash,
    move_skill_to_trash, path_entry_exists, publish_staged_entry_no_replace,
    restore_skill_from_trash_no_displace, validate_direct_skill_path, validate_relative_subpath,
    validate_skill_name, TrashReceipt,
};
use super::skill_store::{SkillStore, SkillTargetRecord};
use super::sync_engine::{copy_dir_recursive, SyncMode};

pub const TOOL_CONFIG_SETTING: &str = "tool_config_v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolId {
    Cursor,
    ClaudeCode,
    Codex,
    DeepSeekHarness,
    OpenCode,
    Antigravity,
    Amp,
    KimiCli,
    Augment,
    OpenClaw,
    Copaw,
    Cline,
    CodeBuddy,
    CodeWhale,
    WorkBuddy,
    CommandCode,
    Continue,
    Crush,
    Junie,
    IflowCli,
    KiroCli,
    Kode,
    McpJam,
    MistralVibe,
    Mux,
    OpenClaude,
    OpenHands,
    Pi,
    Qoder,
    QoderWork,
    QwenCode,
    Trae,
    TraeCn,
    Zencoder,
    Neovate,
    Pochi,
    AdaL,
    KiloCode,
    RooCode,
    Goose,
    GeminiCli,
    GithubCopilot,
    Clawdbot,
    Droid,
    Windsurf,
    Moltbot,
    HermesAgent,
}

impl ToolId {
    pub fn as_key(&self) -> &'static str {
        match self {
            ToolId::Cursor => "cursor",
            ToolId::ClaudeCode => "claude_code",
            ToolId::Codex => "codex",
            ToolId::DeepSeekHarness => "deepseek_harness",
            ToolId::OpenCode => "opencode",
            ToolId::Antigravity => "antigravity",
            ToolId::Amp => "amp",
            ToolId::KimiCli => "kimi_cli",
            ToolId::Augment => "augment",
            ToolId::OpenClaw => "openclaw",
            ToolId::Copaw => "copaw",
            ToolId::Cline => "cline",
            ToolId::CodeBuddy => "codebuddy",
            ToolId::CodeWhale => "codewhale",
            ToolId::WorkBuddy => "workbuddy",
            ToolId::CommandCode => "command_code",
            ToolId::Continue => "continue",
            ToolId::Crush => "crush",
            ToolId::Junie => "junie",
            ToolId::IflowCli => "iflow_cli",
            ToolId::KiroCli => "kiro_cli",
            ToolId::Kode => "kode",
            ToolId::McpJam => "mcpjam",
            ToolId::MistralVibe => "mistral_vibe",
            ToolId::Mux => "mux",
            ToolId::OpenClaude => "openclaude",
            ToolId::OpenHands => "openhands",
            ToolId::Pi => "pi",
            ToolId::Qoder => "qoder",
            ToolId::QoderWork => "qoderwork",
            ToolId::QwenCode => "qwen_code",
            ToolId::Trae => "trae",
            ToolId::TraeCn => "trae_cn",
            ToolId::Zencoder => "zencoder",
            ToolId::Neovate => "neovate",
            ToolId::Pochi => "pochi",
            ToolId::AdaL => "adal",
            ToolId::KiloCode => "kilo_code",
            ToolId::RooCode => "roo_code",
            ToolId::Goose => "goose",
            ToolId::GeminiCli => "gemini_cli",
            ToolId::GithubCopilot => "github_copilot",
            ToolId::Clawdbot => "clawdbot",
            ToolId::Droid => "droid",
            ToolId::Windsurf => "windsurf",
            ToolId::Moltbot => "moltbot",
            ToolId::HermesAgent => "hermes_agent",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ToolAdapter {
    pub id: ToolId,
    pub display_name: &'static str,
    /// Global skill directory under user home (aligned with add-skill docs).
    pub relative_skills_dir: &'static str,
    /// Directory used to detect whether the tool is installed (aligned with add-skill docs).
    pub relative_detect_dir: &'static str,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomToolConfig {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub avatar: Option<String>,
    pub skills_dir: String,
    pub project_skills_dir: Option<String>,
    #[serde(default)]
    pub sync_mode: SyncMode,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ToolConfig {
    #[serde(default)]
    pub disabled_builtin_tools: Vec<String>,
    #[serde(default)]
    pub custom_tools: Vec<CustomToolConfig>,
}

#[derive(Clone, Debug)]
pub struct DetectedSkill {
    pub tool: ToolId,
    pub name: String,
    pub path: PathBuf,
    pub is_link: bool,
    pub link_target: Option<PathBuf>,
}

pub fn load_tool_config(store: &SkillStore) -> Result<ToolConfig> {
    let raw = store.get_setting(TOOL_CONFIG_SETTING)?;
    let config = raw
        .as_deref()
        .and_then(|value| serde_json::from_str::<ToolConfig>(value).ok())
        .unwrap_or_default();
    sanitize_tool_config(config)
}

pub fn save_tool_config(store: &SkillStore, config: ToolConfig) -> Result<ToolConfig> {
    let _mutation_guard = lock_central_mutation()?;
    let previous = load_tool_config(store)?;
    let config = sanitize_tool_config(config)?;
    let serialized = serde_json::to_string(&config)?;
    let plans = preflight_changed_custom_tool_targets(store, &previous, &config)?;
    let created_roots = prepare_custom_tool_roots(&config, &plans)?;
    if let Err(err) = finalize_custom_tool_target_preflight(store, &plans) {
        return rollback_migration_error(&[], &created_roots, err)
            .context("custom tool target preflight failed");
    }

    let mut applied = Vec::new();
    let mut migrated_targets = Vec::new();
    for plan in &plans {
        match apply_custom_tool_target_migration(plan) {
            Ok(Some(journal)) => {
                migrated_targets.push(journal.migrated.clone());
                applied.push(journal);
            }
            Ok(None) => {}
            Err(err) => {
                return rollback_migration_error(&applied, &created_roots, err)
                    .context("custom tool target migration failed");
            }
        }
    }

    if let Err(err) = maybe_replace_applied_custom_tool_target_for_test() {
        return rollback_migration_error(&applied, &created_roots, err)
            .context("custom tool post-apply test hook failed");
    }

    if let Err(err) = store.set_setting_and_skill_targets_atomically(
        TOOL_CONFIG_SETTING,
        &serialized,
        &migrated_targets,
    ) {
        return rollback_migration_error(&applied, &created_roots, err)
            .context("tool config database commit failed");
    }
    Ok(config)
}

#[derive(Clone, Debug)]
struct CustomToolTargetMigrationPlan {
    target: SkillTargetRecord,
    skill_name: String,
    source: PathBuf,
    central_root: PathBuf,
    previous_root: PathBuf,
    next_root: PathBuf,
    previous_target: PathBuf,
    next_target: PathBuf,
    next_mode: SyncMode,
    same_path: bool,
    shared_target: bool,
}

#[derive(Debug)]
struct AppliedCustomToolTargetMigration {
    migrated: SkillTargetRecord,
    previous_root: PathBuf,
    next_root: PathBuf,
    previous_target: PathBuf,
    next_target: PathBuf,
    created_target_identity: TargetEntryIdentity,
    trashed_previous: Option<TrashReceipt>,
}

#[derive(Debug)]
struct StagedCustomToolTarget {
    path: PathBuf,
    identity: TargetEntryIdentity,
    mode_used: SyncMode,
}

#[derive(Debug)]
struct CreatedCustomToolRoot {
    path: PathBuf,
    identity: TargetEntryIdentity,
}

#[derive(Debug, Default)]
struct CreatedCustomToolRoots {
    /// Creation order (parents before children). Rollback always walks this in
    /// reverse so every configured root disappears before a parent created for
    /// it is moved to recoverable Trash.
    paths: Vec<CreatedCustomToolRoot>,
}

#[derive(Debug)]
struct TargetEntryIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    handle: same_file::Handle,
    #[cfg(windows)]
    fingerprint: WindowsEntryFingerprint,
}

#[cfg(windows)]
#[derive(Debug, PartialEq, Eq)]
struct WindowsEntryFingerprint {
    attributes: u32,
    creation_time: u64,
    file_size: u64,
}

impl TargetEntryIdentity {
    fn capture(path: &Path) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            let metadata = std::fs::symlink_metadata(path)
                .with_context(|| format!("capture target entry identity {path:?}"))?;
            Ok(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }

        #[cfg(windows)]
        {
            let handle = same_file::Handle::from_path(path)
                .with_context(|| format!("open target entry identity {path:?}"))?;
            let fingerprint = WindowsEntryFingerprint::capture(path)?;
            Ok(Self {
                handle,
                fingerprint,
            })
        }

        #[cfg(not(any(unix, windows)))]
        anyhow::bail!("TARGET_IDENTITY_UNSUPPORTED|Filesystem entry identity is unavailable")
    }

    fn matches(&self, path: &Path) -> Result<bool> {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => {
                return Err(err).with_context(|| format!("verify target entry identity {path:?}"));
            }
        };

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            Ok(self.device == metadata.dev() && self.inode == metadata.ino())
        }

        #[cfg(windows)]
        {
            let handle = same_file::Handle::from_path(path)
                .with_context(|| format!("open current target entry identity {path:?}"))?;
            Ok(self.handle == handle
                && self.fingerprint == WindowsEntryFingerprint::from_metadata(&metadata))
        }

        #[cfg(not(any(unix, windows)))]
        {
            let _ = metadata;
            Ok(false)
        }
    }
}

#[cfg(windows)]
impl WindowsEntryFingerprint {
    fn capture(path: &Path) -> Result<Self> {
        let metadata = std::fs::symlink_metadata(path)
            .with_context(|| format!("capture Windows target fingerprint {path:?}"))?;
        Ok(Self::from_metadata(&metadata))
    }

    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        use std::os::windows::fs::MetadataExt;

        Self {
            attributes: metadata.file_attributes(),
            creation_time: metadata.creation_time(),
            file_size: metadata.file_size(),
        }
    }
}

fn preflight_changed_custom_tool_targets(
    store: &SkillStore,
    previous: &ToolConfig,
    next: &ToolConfig,
) -> Result<Vec<CustomToolTargetMigrationPlan>> {
    let previous_by_key = previous
        .custom_tools
        .iter()
        .map(|tool| (tool.key.as_str(), tool))
        .collect::<HashMap<_, _>>();
    let mut plans = Vec::new();
    let mut planned_destinations: Vec<PathBuf> = Vec::new();

    for next_tool in &next.custom_tools {
        let Some(previous_tool) = previous_by_key.get(next_tool.key.as_str()) else {
            continue;
        };
        if previous_tool.skills_dir == next_tool.skills_dir
            && previous_tool.project_skills_dir == next_tool.project_skills_dir
            && previous_tool.sync_mode == next_tool.sync_mode
        {
            continue;
        }

        for target in store.list_skill_targets_by_tool(&next_tool.key)? {
            if target.status == "disabled" {
                continue;
            }
            let target_changed = match target.scope.as_str() {
                "global" => {
                    previous_tool.skills_dir != next_tool.skills_dir
                        || previous_tool.sync_mode != next_tool.sync_mode
                }
                "project" => {
                    previous_tool.project_skills_dir != next_tool.project_skills_dir
                        || previous_tool.sync_mode != next_tool.sync_mode
                }
                _ => true,
            };
            if !target_changed {
                continue;
            }
            let plan = preflight_custom_tool_target(store, previous_tool, next_tool, target)?;
            if !plan.same_path {
                for destination in &planned_destinations {
                    if physical_target_entries_match(destination, &plan.next_target)? {
                        anyhow::bail!(
                            "CUSTOM_TOOL_TARGET_CONFLICT|multiple targets resolve to {:?}",
                            plan.next_target
                        );
                    }
                }
                planned_destinations.push(plan.next_target.clone());
            }
            plans.push(plan);
        }
    }

    Ok(plans)
}

fn preflight_custom_tool_target(
    store: &SkillStore,
    previous_tool: &CustomToolConfig,
    next_tool: &CustomToolConfig,
    target: SkillTargetRecord,
) -> Result<CustomToolTargetMigrationPlan> {
    let skill = store
        .get_skill_by_id(&target.skill_id)?
        .with_context(|| format!("skill not found for target {}", target.id))?;
    let source = PathBuf::from(&skill.central_path);
    if !source.is_dir() {
        anyhow::bail!("managed skill directory not found: {:?}", source);
    }
    validate_skill_name(&skill.name)?;

    #[cfg(not(test))]
    let central_root = dirs::home_dir()
        .context("failed to resolve home directory")?
        .join(".agents/skills");
    #[cfg(test)]
    let central_root = source
        .parent()
        .context("managed test Skill has no central root")?
        .to_path_buf();
    validate_direct_skill_path(&central_root, &source)?;

    let previous_root = custom_tool_target_root(previous_tool, &target)?;
    let next_root = custom_tool_target_root(next_tool, &target)?;
    if !previous_root.is_dir() {
        anyhow::bail!("custom tool Skill root not found: {:?}", previous_root);
    }
    ensure_distinct_roots(&previous_root, &central_root)?;
    let previous_target = PathBuf::from(&target.target_path);
    if previous_target
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        != Some(skill.name.as_str())
    {
        anyhow::bail!("UNSAFE_PATH|Stored custom target name does not match managed Skill");
    }
    validate_direct_skill_path(&previous_root, &previous_target)?;
    let next_target = next_root.join(&skill.name);
    let same_path = next_target == previous_target
        || (next_root.is_dir()
            && previous_root.canonicalize().ok() == next_root.canonicalize().ok());
    let shared_target =
        is_physical_target_used_by_another_record(store, &previous_target, &target.id)?;

    if !same_path && path_entry_exists(&next_target)? {
        anyhow::bail!(
            "CUSTOM_TOOL_TARGET_CONFLICT|new custom tool target already exists: {:?}",
            next_target
        );
    }
    if !same_path && is_physical_target_used_by_another_record(store, &next_target, &target.id)? {
        anyhow::bail!(
            "CUSTOM_TOOL_TARGET_CONFLICT|new custom tool target is already managed: {:?}",
            next_target
        );
    }

    Ok(CustomToolTargetMigrationPlan {
        target,
        skill_name: skill.name,
        source,
        central_root,
        previous_root,
        next_root,
        previous_target,
        next_target,
        next_mode: next_tool.sync_mode,
        same_path,
        shared_target,
    })
}

fn custom_tool_target_root(tool: &CustomToolConfig, target: &SkillTargetRecord) -> Result<PathBuf> {
    if target.scope == "project" {
        let project_path = target
            .project_path
            .as_deref()
            .context("project target is missing its project path")?;
        let relative = tool.project_skills_dir.as_deref().with_context(|| {
            format!(
                "custom tool {} still has project Skills synced; set a project directory before saving",
                tool.label
            )
        })?;
        Ok(PathBuf::from(project_path).join(relative))
    } else if target.scope == "global" {
        expand_custom_tool_path(&tool.skills_dir)
    } else {
        anyhow::bail!("UNSAFE_PATH|Unknown target scope {}", target.scope)
    }
}

/// Target rows are aliases when their parent directories resolve to the same
/// physical tool root and they name the same Skill child. Deliberately avoid
/// canonicalizing the full target: two distinct symlink entries that happen
/// to point at the same central Skill are separate managed targets.
fn physical_target_entries_match(left: &Path, right: &Path) -> Result<bool> {
    if left == right {
        return Ok(true);
    }
    if left.file_name().is_none() || left.file_name() != right.file_name() {
        return Ok(false);
    }
    let Some(left_parent) = left.parent() else {
        return Ok(false);
    };
    let Some(right_parent) = right.parent() else {
        return Ok(false);
    };
    let Some(left_identity) = canonicalize_optional_parent(left_parent)? else {
        return Ok(false);
    };
    let Some(right_identity) = canonicalize_optional_parent(right_parent)? else {
        return Ok(false);
    };
    Ok(left_identity == right_identity)
}

fn canonicalize_optional_parent(path: &Path) -> Result<Option<PathBuf>> {
    match path.canonicalize() {
        Ok(path) => Ok(Some(path)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("resolve target parent {path:?}")),
    }
}

fn is_physical_target_used_by_another_record(
    store: &SkillStore,
    target: &Path,
    record_id: &str,
) -> Result<bool> {
    if store.is_skill_target_path_used_by_another_record(&target.to_string_lossy(), record_id)? {
        return Ok(true);
    }
    for other in store.list_active_skill_target_paths_except(record_id)? {
        if physical_target_entries_match(target, Path::new(&other))? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn prepare_custom_tool_roots(
    config: &ToolConfig,
    plans: &[CustomToolTargetMigrationPlan],
) -> Result<CreatedCustomToolRoots> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    for tool in &config.custom_tools {
        if tool.enabled {
            let root = expand_custom_tool_path(&tool.skills_dir)?;
            if seen.insert(root.clone()) {
                roots.push(root);
            }
        }
    }
    for plan in plans {
        if seen.insert(plan.next_root.clone()) {
            roots.push(plan.next_root.clone());
        }
    }

    let mut created = CreatedCustomToolRoots::default();
    for root in roots {
        if let Err(err) = create_custom_tool_root_with_journal(&root, &mut created) {
            return rollback_migration_error(&[], &created, err)
                .context("create custom tool Skill roots");
        }
    }
    Ok(created)
}

fn create_custom_tool_root_with_journal(
    root: &Path,
    created: &mut CreatedCustomToolRoots,
) -> Result<()> {
    if !root.is_absolute() {
        anyhow::bail!("custom tool skills directory must be absolute");
    }
    if root.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    }) {
        anyhow::bail!("UNSAFE_PATH|Custom tool root must not contain . or .. components");
    }

    let mut missing = Vec::new();
    let mut cursor = root;
    loop {
        match std::fs::symlink_metadata(cursor) {
            Ok(_) => break,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let name = cursor
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .context("custom tool Skill root has no UTF-8 name")?;
                if name == ".system" || name.chars().any(char::is_control) {
                    anyhow::bail!("UNSAFE_PATH|Custom tool root cannot be rolled back safely");
                }
                missing.push(cursor.to_path_buf());
                cursor = cursor
                    .parent()
                    .context("custom tool Skill root has no existing ancestor")?;
            }
            Err(err) => {
                return Err(err).with_context(|| format!("inspect custom tool root {cursor:?}"));
            }
        }
    }

    for path in missing.iter().rev() {
        match std::fs::create_dir(path) {
            Ok(()) => {
                let identity = TargetEntryIdentity::capture(path)?;
                created.paths.push(CreatedCustomToolRoot {
                    path: path.clone(),
                    identity,
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => {}
            Err(err) => {
                return Err(err).with_context(|| format!("create custom tool Skill root {path:?}"));
            }
        }
    }
    if !root.is_dir() {
        anyhow::bail!("custom tool Skill root is not a directory: {root:?}");
    }
    Ok(())
}

fn finalize_custom_tool_target_preflight(
    store: &SkillStore,
    plans: &[CustomToolTargetMigrationPlan],
) -> Result<()> {
    let mut destinations: Vec<PathBuf> = Vec::new();
    for plan in plans {
        ensure_distinct_roots(&plan.next_root, &plan.central_root)?;
        let expected_target = direct_skill_child(&plan.next_root, &plan.skill_name)?;
        if expected_target != plan.next_target {
            anyhow::bail!("UNSAFE_PATH|Custom tool target changed during preflight");
        }
        if !plan.same_path && path_entry_exists(&plan.next_target)? {
            anyhow::bail!(
                "CUSTOM_TOOL_TARGET_CONFLICT|new custom tool target already exists: {:?}",
                plan.next_target
            );
        }
        if !plan.same_path {
            if is_physical_target_used_by_another_record(store, &plan.next_target, &plan.target.id)?
            {
                anyhow::bail!(
                    "CUSTOM_TOOL_TARGET_CONFLICT|new custom tool target is already managed: {:?}",
                    plan.next_target
                );
            }
            for destination in &destinations {
                if physical_target_entries_match(destination, &plan.next_target)? {
                    anyhow::bail!(
                        "CUSTOM_TOOL_TARGET_CONFLICT|multiple targets resolve to {:?}",
                        plan.next_target
                    );
                }
            }
            destinations.push(plan.next_target.clone());
        }
    }
    Ok(())
}

fn prepare_custom_tool_staged_target(
    plan: &CustomToolTargetMigrationPlan,
) -> Result<StagedCustomToolTarget> {
    match plan.next_mode {
        SyncMode::Copy => prepare_copy_staging(plan),
        SyncMode::Symlink => try_prepare_symlink_staging(plan)?
            .context("SYNC_MODE_UNAVAILABLE|Could not create the requested directory symlink"),
        SyncMode::Junction => try_prepare_junction_staging(plan)?
            .context("SYNC_MODE_UNAVAILABLE|Could not create the requested directory junction"),
        SyncMode::Auto => {
            if let Some(staged) = try_prepare_symlink_staging(plan)? {
                return Ok(staged);
            }
            #[cfg(windows)]
            if let Some(staged) = try_prepare_junction_staging(plan)? {
                return Ok(staged);
            }
            prepare_copy_staging(plan)
        }
    }
}

fn prepare_copy_staging(plan: &CustomToolTargetMigrationPlan) -> Result<StagedCustomToolTarget> {
    for _ in 0..8 {
        let staged = unique_custom_tool_staging_path(&plan.next_root);
        match std::fs::create_dir(&staged) {
            Ok(()) => {
                let identity = TargetEntryIdentity::capture(&staged).with_context(|| {
                    format!(
                        "ROLLBACK_INCOMPLETE|Could not identify newly created staging directory {staged:?}"
                    )
                })?;
                if let Err(err) = copy_dir_recursive(&plan.source, &staged) {
                    return staged_preparation_failure(plan, &staged, &identity, err)
                        .context("copy managed Skill into transaction staging");
                }
                return Ok(StagedCustomToolTarget {
                    path: staged,
                    identity,
                    mode_used: SyncMode::Copy,
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(err).with_context(|| format!("create copy staging {staged:?}"));
            }
        }
    }
    anyhow::bail!("STAGING_COLLISION|Could not allocate custom tool transaction staging")
}

fn try_prepare_symlink_staging(
    plan: &CustomToolTargetMigrationPlan,
) -> Result<Option<StagedCustomToolTarget>> {
    for _ in 0..8 {
        let staged = unique_custom_tool_staging_path(&plan.next_root);
        match create_directory_symlink(&plan.source, &staged) {
            Ok(()) => {
                let identity = TargetEntryIdentity::capture(&staged).with_context(|| {
                    format!(
                        "ROLLBACK_INCOMPLETE|Could not identify newly created symlink staging {staged:?}"
                    )
                })?;
                return Ok(Some(StagedCustomToolTarget {
                    path: staged,
                    identity,
                    mode_used: SyncMode::Symlink,
                }));
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                if path_entry_exists(&staged)? {
                    anyhow::bail!(
                        "ROLLBACK_INCOMPLETE|symlink staging failed ({err}); an unowned entry appeared at {staged:?}"
                    );
                }
                log::debug!(
                    "[tool_adapters] directory symlink unavailable for {:?}: {}",
                    plan.next_target,
                    err
                );
                return Ok(None);
            }
        }
    }
    anyhow::bail!("STAGING_COLLISION|Could not allocate symlink transaction staging")
}

fn try_prepare_junction_staging(
    plan: &CustomToolTargetMigrationPlan,
) -> Result<Option<StagedCustomToolTarget>> {
    for _ in 0..8 {
        let staged = unique_custom_tool_staging_path(&plan.next_root);
        match create_directory_junction(&plan.source, &staged) {
            Ok(()) => {
                let identity = TargetEntryIdentity::capture(&staged).with_context(|| {
                    format!(
                        "ROLLBACK_INCOMPLETE|Could not identify newly created junction staging {staged:?}"
                    )
                })?;
                return Ok(Some(StagedCustomToolTarget {
                    path: staged,
                    identity,
                    mode_used: SyncMode::Junction,
                }));
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                if path_entry_exists(&staged)? {
                    anyhow::bail!(
                        "ROLLBACK_INCOMPLETE|junction staging failed ({err}); an unowned or partial entry remains at {staged:?}"
                    );
                }
                log::debug!(
                    "[tool_adapters] directory junction unavailable for {:?}: {}",
                    plan.next_target,
                    err
                );
                return Ok(None);
            }
        }
    }
    anyhow::bail!("STAGING_COLLISION|Could not allocate junction transaction staging")
}

fn unique_custom_tool_staging_path(root: &Path) -> PathBuf {
    root.join(format!(
        ".skills-hub-custom-tool-{}",
        Uuid::new_v4().simple()
    ))
}

#[cfg(unix)]
fn create_directory_symlink(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(windows)]
fn create_directory_symlink(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(source, target)
}

#[cfg(not(any(unix, windows)))]
fn create_directory_symlink(_source: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "directory symlinks are unsupported",
    ))
}

#[cfg(windows)]
fn create_directory_junction(source: &Path, target: &Path) -> std::io::Result<()> {
    junction::create(source, target)
}

#[cfg(not(windows))]
fn create_directory_junction(_source: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "directory junctions are unsupported",
    ))
}

fn staged_preparation_failure<T>(
    plan: &CustomToolTargetMigrationPlan,
    staged: &Path,
    identity: &TargetEntryIdentity,
    original: anyhow::Error,
) -> Result<T> {
    match cleanup_owned_internal_entry(&plan.next_root, staged, identity) {
        Ok(()) => Err(original),
        Err(cleanup) => anyhow::bail!(
            "ROLLBACK_INCOMPLETE|staging failed ({original:#}); staging cleanup failed: {cleanup:#}"
        ),
    }
}

fn publish_custom_tool_staged_target(
    plan: &CustomToolTargetMigrationPlan,
    staged: &StagedCustomToolTarget,
) -> Result<()> {
    maybe_create_competing_custom_tool_target_for_test(&plan.target.id, &plan.next_target)?;
    match publish_staged_entry_no_replace(&plan.next_root, &staged.path, &plan.next_target) {
        Ok(()) => {
            if staged.identity.matches(&plan.next_target)? {
                Ok(())
            } else {
                anyhow::bail!(
                    "ROLLBACK_INCOMPLETE|Published target identity changed before it could be journaled: {:?}",
                    plan.next_target
                )
            }
        }
        Err(original) => {
            let cleanup = match staged.identity.matches(&plan.next_target) {
                Ok(true) => {
                    cleanup_owned_skill_entry(&plan.next_root, &plan.next_target, &staged.identity)
                }
                Ok(false) => {
                    cleanup_owned_internal_entry(&plan.next_root, &staged.path, &staged.identity)
                }
                Err(err) => Err(err),
            };
            match cleanup {
                Ok(()) => Err(original),
                Err(cleanup) => anyhow::bail!(
                    "ROLLBACK_INCOMPLETE|atomic publish failed ({original:#}); owned staging cleanup failed: {cleanup:#}"
                ),
            }
        }
    }
}

fn cleanup_owned_skill_entry(
    root: &Path,
    path: &Path,
    identity: &TargetEntryIdentity,
) -> Result<()> {
    cleanup_owned_entry(root, path, identity, false)
}

fn cleanup_owned_internal_entry(
    root: &Path,
    path: &Path,
    identity: &TargetEntryIdentity,
) -> Result<()> {
    cleanup_owned_entry(root, path, identity, true)
}

fn cleanup_owned_entry(
    root: &Path,
    path: &Path,
    identity: &TargetEntryIdentity,
    internal: bool,
) -> Result<()> {
    if !path_entry_exists(path)? {
        return Ok(());
    }
    if !identity.matches(path)? {
        anyhow::bail!(
            "ROLLBACK_SKIPPED_FOREIGN_ENTRY|Preserved an entry whose identity does not match this transaction: {path:?}"
        );
    }
    if internal {
        move_internal_to_trash(root, path)?;
    } else {
        move_skill_to_trash(root, path)?;
    }
    Ok(())
}

fn apply_custom_tool_target_migration(
    plan: &CustomToolTargetMigrationPlan,
) -> Result<Option<AppliedCustomToolTargetMigration>> {
    // A shared physical target must keep its current representation because
    // changing it would also mutate the other tool's live target. The selected
    // mode still applies to future paths through the committed tool config.
    if plan.same_path && plan.shared_target {
        return Ok(None);
    }

    maybe_fail_custom_tool_target_for_test(&plan.target.id)?;

    let staged = prepare_custom_tool_staged_target(plan).with_context(|| {
        format!(
            "prepare custom tool target transaction {:?} -> {:?}",
            plan.previous_target, plan.next_target
        )
    })?;

    let trashed_previous = if plan.same_path {
        match move_skill_to_trash(&plan.previous_root, &plan.previous_target) {
            Ok(backup) => backup,
            Err(err) => {
                return staged_preparation_failure(plan, &staged.path, &staged.identity, err)
                    .context("move old custom tool target to Trash");
            }
        }
    } else {
        None
    };

    match publish_custom_tool_staged_target(plan, &staged).with_context(|| {
        format!(
            "publish custom tool target {:?} -> {:?}",
            plan.previous_target, plan.next_target
        )
    }) {
        Ok(()) => {}
        Err(err) => {
            return rollback_current_migration_failure(plan, None, trashed_previous.as_ref(), err);
        }
    }

    let trashed_previous = if !plan.same_path && !plan.shared_target {
        match move_skill_to_trash(&plan.previous_root, &plan.previous_target) {
            Ok(backup) => backup,
            Err(err) => {
                return rollback_current_migration_failure(plan, Some(&staged.identity), None, err)
                    .context("move old custom tool target to Trash");
            }
        }
    } else {
        trashed_previous
    };

    let migrated = SkillTargetRecord {
        id: plan.target.id.clone(),
        skill_id: plan.target.skill_id.clone(),
        tool: plan.target.tool.clone(),
        scope: plan.target.scope.clone(),
        project_path: plan.target.project_path.clone(),
        target_path: plan.next_target.to_string_lossy().to_string(),
        mode: sync_mode_key(staged.mode_used).to_string(),
        status: "ok".to_string(),
        last_error: None,
        synced_at: Some(current_time_ms()),
    };

    Ok(Some(AppliedCustomToolTargetMigration {
        migrated,
        previous_root: plan.previous_root.clone(),
        next_root: plan.next_root.clone(),
        previous_target: plan.previous_target.clone(),
        next_target: plan.next_target.clone(),
        created_target_identity: staged.identity,
        trashed_previous,
    }))
}

fn rollback_current_migration_failure<T>(
    plan: &CustomToolTargetMigrationPlan,
    created_target_identity: Option<&TargetEntryIdentity>,
    trashed_previous: Option<&TrashReceipt>,
    original: anyhow::Error,
) -> Result<T> {
    let cleanup_result = match created_target_identity {
        Some(identity) => cleanup_owned_skill_entry(&plan.next_root, &plan.next_target, identity),
        None => Ok(()),
    };
    let restore_result = if let Some(backup) = trashed_previous {
        restore_trashed_previous_without_displacing(
            &plan.previous_root,
            &plan.previous_target,
            backup,
        )
    } else {
        Ok(())
    };

    match (cleanup_result, restore_result) {
        (Ok(()), Ok(())) => Err(original),
        (cleanup, restore) => anyhow::bail!(
            "ROLLBACK_INCOMPLETE|migration failed ({original:#}); remove new target: {}; restore old target: {}",
            rollback_result_label(cleanup),
            rollback_result_label(restore)
        ),
    }
}

fn rollback_applied_custom_tool_migrations(
    applied: &[AppliedCustomToolTargetMigration],
) -> Result<()> {
    let mut errors = Vec::new();
    for journal in applied.iter().rev() {
        if let Err(err) = cleanup_owned_skill_entry(
            &journal.next_root,
            &journal.next_target,
            &journal.created_target_identity,
        ) {
            errors.push(format!("remove {:?}: {err:#}", journal.next_target));
        }
        if let Some(backup) = journal.trashed_previous.as_ref() {
            if let Err(err) = restore_trashed_previous_without_displacing(
                &journal.previous_root,
                &journal.previous_target,
                backup,
            ) {
                errors.push(format!("restore {:?}: {err:#}", journal.previous_target));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("ROLLBACK_INCOMPLETE|{}", errors.join(" | "))
    }
}

fn restore_trashed_previous_without_displacing(
    root: &Path,
    target: &Path,
    backup: &TrashReceipt,
) -> Result<()> {
    restore_skill_from_trash_no_displace(root, target, backup)
}

fn rollback_created_custom_tool_roots(created: &CreatedCustomToolRoots) -> Result<()> {
    let mut errors = Vec::new();
    for root in created.paths.iter().rev() {
        match path_entry_exists(&root.path) {
            Ok(true) => {
                match root.identity.matches(&root.path) {
                    Ok(true) => {}
                    Ok(false) => {
                        errors.push(format!(
                            "preserved replaced custom tool root {:?}",
                            root.path
                        ));
                        continue;
                    }
                    Err(err) => {
                        errors.push(format!("verify root {:?}: {err:#}", root.path));
                        continue;
                    }
                }
                match created_root_is_safe_to_rollback(&root.path) {
                    Ok(true) => {}
                    Ok(false) => {
                        errors.push(format!(
                            "preserved newly populated custom tool root {:?}",
                            root.path
                        ));
                        continue;
                    }
                    Err(err) => {
                        errors.push(format!("inspect root {:?}: {err:#}", root.path));
                        continue;
                    }
                }
                let Some(parent) = root.path.parent() else {
                    errors.push(format!("root has no parent: {:?}", root.path));
                    continue;
                };
                if let Err(err) = move_internal_to_trash(parent, &root.path) {
                    errors.push(format!(
                        "remove newly created root {:?}: {err:#}",
                        root.path
                    ));
                }
            }
            Ok(false) => {}
            Err(err) => errors.push(format!(
                "inspect newly created root {:?}: {err:#}",
                root.path
            )),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("ROLLBACK_INCOMPLETE|{}", errors.join(" | "))
    }
}

fn created_root_is_safe_to_rollback(path: &Path) -> Result<bool> {
    #[cfg(test)]
    let entries =
        std::fs::read_dir(path).with_context(|| format!("read newly created root {path:?}"))?;
    #[cfg(not(test))]
    let mut entries =
        std::fs::read_dir(path).with_context(|| format!("read newly created root {path:?}"))?;
    #[cfg(test)]
    {
        for entry in entries {
            let entry = entry?;
            if entry.file_name() == ".skills-hub-test-trash" {
                continue;
            }
            return Ok(false);
        }
        Ok(true)
    }
    #[cfg(not(test))]
    {
        Ok(entries.next().transpose()?.is_none())
    }
}

fn rollback_migration_error<T>(
    applied: &[AppliedCustomToolTargetMigration],
    created_roots: &CreatedCustomToolRoots,
    original: anyhow::Error,
) -> Result<T> {
    let targets = rollback_applied_custom_tool_migrations(applied);
    let roots = rollback_created_custom_tool_roots(created_roots);
    match (targets, roots) {
        (Ok(()), Ok(())) => Err(original),
        (targets, roots) => anyhow::bail!(
            "ROLLBACK_INCOMPLETE|operation failed ({original:#}); target rollback: {}; root rollback: {}",
            rollback_result_label(targets),
            rollback_result_label(roots)
        ),
    }
}

fn rollback_result_label(result: Result<()>) -> String {
    match result {
        Ok(()) => "ok".to_string(),
        Err(err) => format!("{err:#}"),
    }
}

#[cfg(test)]
thread_local! {
    static TEST_FAIL_CUSTOM_TOOL_TARGET: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    static TEST_RACE_CUSTOM_TOOL_TARGET: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    static TEST_REPLACE_APPLIED_CUSTOM_TOOL_TARGET: std::cell::RefCell<Option<(PathBuf, PathBuf)>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_test_custom_tool_migration_failure(target_id: Option<&str>) {
    TEST_FAIL_CUSTOM_TOOL_TARGET.with(|slot| {
        *slot.borrow_mut() = target_id.map(str::to_string);
    });
}

#[cfg(test)]
pub(crate) fn set_test_custom_tool_publish_race(target_id: Option<&str>) {
    TEST_RACE_CUSTOM_TOOL_TARGET.with(|slot| {
        *slot.borrow_mut() = target_id.map(str::to_string);
    });
}

#[cfg(test)]
pub(crate) fn set_test_custom_tool_post_apply_replacement(live: &Path, preserved: &Path) {
    TEST_REPLACE_APPLIED_CUSTOM_TOOL_TARGET.with(|slot| {
        *slot.borrow_mut() = Some((live.to_path_buf(), preserved.to_path_buf()));
    });
}

#[cfg(test)]
fn maybe_fail_custom_tool_target_for_test(target_id: &str) -> Result<()> {
    TEST_FAIL_CUSTOM_TOOL_TARGET.with(|slot| {
        let should_fail = slot.borrow().as_deref() == Some(target_id);
        if should_fail {
            slot.borrow_mut().take();
            anyhow::bail!("TEST_MIGRATION_FAILURE|{target_id}");
        }
        Ok(())
    })
}

#[cfg(test)]
fn maybe_create_competing_custom_tool_target_for_test(
    target_id: &str,
    target: &Path,
) -> Result<()> {
    TEST_RACE_CUSTOM_TOOL_TARGET.with(|slot| {
        let should_race = slot.borrow().as_deref() == Some(target_id);
        if !should_race {
            return Ok(());
        }
        slot.borrow_mut().take();
        std::fs::create_dir(target)
            .with_context(|| format!("TEST_RACE_CREATE_FAILED|create competitor {target:?}"))?;
        std::fs::write(target.join("COMPETITOR_SENTINEL"), "external owner")
            .context("TEST_RACE_CREATE_FAILED|write competitor sentinel")?;
        Ok(())
    })
}

#[cfg(test)]
fn maybe_replace_applied_custom_tool_target_for_test() -> Result<()> {
    TEST_REPLACE_APPLIED_CUSTOM_TOOL_TARGET.with(|slot| {
        let Some((live, preserved)) = slot.borrow_mut().take() else {
            return Ok(());
        };
        std::fs::rename(&live, &preserved).with_context(|| {
            format!("TEST_POST_APPLY_RACE_FAILED|preserve published target {live:?}")
        })?;
        std::fs::create_dir(&live)
            .with_context(|| format!("TEST_POST_APPLY_RACE_FAILED|create competitor {live:?}"))?;
        std::fs::write(live.join("COMPETITOR_SENTINEL"), "external owner")
            .context("TEST_POST_APPLY_RACE_FAILED|write competitor sentinel")?;
        Ok(())
    })
}

#[cfg(not(test))]
fn maybe_fail_custom_tool_target_for_test(_target_id: &str) -> Result<()> {
    Ok(())
}

#[cfg(not(test))]
fn maybe_create_competing_custom_tool_target_for_test(
    _target_id: &str,
    _target: &Path,
) -> Result<()> {
    Ok(())
}

#[cfg(not(test))]
fn maybe_replace_applied_custom_tool_target_for_test() -> Result<()> {
    Ok(())
}

fn sync_mode_key(mode: SyncMode) -> &'static str {
    match mode {
        SyncMode::Auto => "auto",
        SyncMode::Symlink => "symlink",
        SyncMode::Junction => "junction",
        SyncMode::Copy => "copy",
    }
}

fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub fn is_builtin_tool_enabled(config: &ToolConfig, key: &str) -> bool {
    !config
        .disabled_builtin_tools
        .iter()
        .any(|disabled| disabled == key)
}

pub fn is_valid_custom_tool_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn expand_custom_tool_path(input: &str) -> Result<PathBuf> {
    let trimmed = input.trim();
    if trimmed == "~" {
        return dirs::home_dir().context("failed to resolve home directory");
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        let home = dirs::home_dir().context("failed to resolve home directory")?;
        return Ok(home.join(rest));
    }
    Ok(PathBuf::from(trimmed))
}

fn sanitize_tool_config(mut config: ToolConfig) -> Result<ToolConfig> {
    let builtin_keys = default_tool_adapters()
        .into_iter()
        .map(|adapter| adapter.id.as_key().to_string())
        .collect::<std::collections::HashSet<_>>();

    config
        .disabled_builtin_tools
        .retain(|key| builtin_keys.contains(key));
    config.disabled_builtin_tools.sort();
    config.disabled_builtin_tools.dedup();

    let mut seen_custom_keys = std::collections::HashSet::new();
    let mut custom_tools = Vec::new();
    for mut tool in config.custom_tools {
        tool.key = tool.key.trim().to_string();
        tool.label = tool.label.trim().to_string();
        tool.avatar = tool
            .avatar
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        tool.skills_dir = tool.skills_dir.trim().to_string();
        tool.project_skills_dir = tool
            .project_skills_dir
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if tool.key.is_empty() {
            anyhow::bail!("custom tool key is required");
        }
        if builtin_keys.contains(&tool.key) {
            anyhow::bail!(
                "custom tool key conflicts with a built-in tool: {}",
                tool.key
            );
        }
        if !is_valid_custom_tool_key(&tool.key) {
            anyhow::bail!("custom tool key contains invalid characters: {}", tool.key);
        }
        if !seen_custom_keys.insert(tool.key.clone()) {
            anyhow::bail!("duplicate custom tool key: {}", tool.key);
        }
        if tool.label.is_empty() {
            anyhow::bail!("custom tool name is required");
        }
        if tool.skills_dir.is_empty() {
            anyhow::bail!("custom tool skills directory is required");
        }
        if !expand_custom_tool_path(&tool.skills_dir)?.is_absolute() {
            anyhow::bail!("custom tool skills directory must be absolute or start with ~/");
        }
        if let Some(relative) = tool.project_skills_dir.as_deref() {
            validate_relative_subpath(relative)?;
        }
        if let Some(avatar) = &tool.avatar {
            if !avatar.starts_with("data:image/") {
                anyhow::bail!("custom tool avatar must be an image data URL");
            }
            if avatar.len() > 512 * 1024 {
                anyhow::bail!("custom tool avatar is too large");
            }
        }
        custom_tools.push(tool);
    }
    config.custom_tools = custom_tools;

    Ok(config)
}

pub fn default_tool_adapters() -> Vec<ToolAdapter> {
    vec![
        ToolAdapter {
            id: ToolId::Cursor,
            display_name: "Cursor",
            relative_skills_dir: ".cursor/skills",
            relative_detect_dir: ".cursor",
        },
        ToolAdapter {
            id: ToolId::ClaudeCode,
            display_name: "Claude Code",
            relative_skills_dir: ".claude/skills",
            relative_detect_dir: ".claude",
        },
        ToolAdapter {
            id: ToolId::Codex,
            display_name: "Codex",
            relative_skills_dir: ".codex/skills",
            relative_detect_dir: ".codex",
        },
        ToolAdapter {
            id: ToolId::DeepSeekHarness,
            display_name: "DeepSeek Harness",
            // DeepSeek Harness default DSH_HOME is ~/.dsh.
            relative_skills_dir: ".dsh/skills",
            relative_detect_dir: ".dsh",
        },
        ToolAdapter {
            id: ToolId::OpenCode,
            display_name: "OpenCode",
            // add-skill global path: ~/.config/opencode/skills/
            relative_skills_dir: ".config/opencode/skills",
            relative_detect_dir: ".config/opencode",
        },
        ToolAdapter {
            id: ToolId::Antigravity,
            display_name: "Antigravity",
            // Antigravity 2.0 global path: ~/.gemini/config/skills/
            relative_skills_dir: ".gemini/config/skills",
            relative_detect_dir: ".gemini/config",
        },
        ToolAdapter {
            id: ToolId::Amp,
            display_name: "Amp",
            // add-skill global path: ~/.config/agents/skills/
            relative_skills_dir: ".config/agents/skills",
            relative_detect_dir: ".config/agents",
        },
        ToolAdapter {
            id: ToolId::KimiCli,
            display_name: "Kimi Code CLI",
            // add-skill global path: ~/.config/agents/skills/
            // NOTE: Shares the same skills directory with Amp.
            relative_skills_dir: ".config/agents/skills",
            relative_detect_dir: ".config/agents",
        },
        ToolAdapter {
            id: ToolId::Augment,
            display_name: "Augment",
            // add-skill global path: ~/.augment/skills/
            relative_skills_dir: ".augment/skills",
            relative_detect_dir: ".augment",
        },
        ToolAdapter {
            id: ToolId::OpenClaw,
            display_name: "OpenClaw",
            // add-skill global path: ~/.openclaw/skills/
            relative_skills_dir: ".openclaw/skills",
            relative_detect_dir: ".openclaw",
        },
        ToolAdapter {
            id: ToolId::Copaw,
            display_name: "Copaw",
            // add-skill global path: ~/.copaw/skill_pool/
            relative_skills_dir: ".copaw/skill_pool",
            relative_detect_dir: ".copaw",
        },
        ToolAdapter {
            id: ToolId::Cline,
            display_name: "Cline",
            // add-skill global path: ~/.cline/skills/
            relative_skills_dir: ".cline/skills",
            relative_detect_dir: ".cline",
        },
        ToolAdapter {
            id: ToolId::CodeBuddy,
            display_name: "CodeBuddy",
            // add-skill global path: ~/.codebuddy/skills/
            relative_skills_dir: ".codebuddy/skills",
            relative_detect_dir: ".codebuddy",
        },
        ToolAdapter {
            id: ToolId::CodeWhale,
            display_name: "CodeWhale",
            // CodeWhale default global path: ~/.codewhale/skills/
            relative_skills_dir: ".codewhale/skills",
            relative_detect_dir: ".codewhale",
        },
        ToolAdapter {
            id: ToolId::WorkBuddy,
            display_name: "WorkBuddy",
            // add-skill global path: ~/.workbuddy/skills/
            relative_skills_dir: ".workbuddy/skills",
            relative_detect_dir: ".workbuddy",
        },
        ToolAdapter {
            id: ToolId::CommandCode,
            display_name: "Command Code",
            // add-skill global path: ~/.commandcode/skills/
            relative_skills_dir: ".commandcode/skills",
            relative_detect_dir: ".commandcode",
        },
        ToolAdapter {
            id: ToolId::Continue,
            display_name: "Continue",
            // add-skill global path: ~/.continue/skills/
            relative_skills_dir: ".continue/skills",
            relative_detect_dir: ".continue",
        },
        ToolAdapter {
            id: ToolId::Crush,
            display_name: "Crush",
            // add-skill global path: ~/.config/crush/skills/
            relative_skills_dir: ".config/crush/skills",
            relative_detect_dir: ".config/crush",
        },
        ToolAdapter {
            id: ToolId::Junie,
            display_name: "Junie",
            // add-skill global path: ~/.junie/skills/
            relative_skills_dir: ".junie/skills",
            relative_detect_dir: ".junie",
        },
        ToolAdapter {
            id: ToolId::IflowCli,
            display_name: "iFlow CLI",
            // add-skill global path: ~/.iflow/skills/
            relative_skills_dir: ".iflow/skills",
            relative_detect_dir: ".iflow",
        },
        ToolAdapter {
            id: ToolId::KiroCli,
            display_name: "Kiro CLI",
            // add-skill global path: ~/.kiro/skills/
            relative_skills_dir: ".kiro/skills",
            relative_detect_dir: ".kiro",
        },
        ToolAdapter {
            id: ToolId::Kode,
            display_name: "Kode",
            // add-skill global path: ~/.kode/skills/
            relative_skills_dir: ".kode/skills",
            relative_detect_dir: ".kode",
        },
        ToolAdapter {
            id: ToolId::McpJam,
            display_name: "MCPJam",
            // add-skill global path: ~/.mcpjam/skills/
            relative_skills_dir: ".mcpjam/skills",
            relative_detect_dir: ".mcpjam",
        },
        ToolAdapter {
            id: ToolId::MistralVibe,
            display_name: "Mistral Vibe",
            // add-skill global path: ~/.vibe/skills/
            relative_skills_dir: ".vibe/skills",
            relative_detect_dir: ".vibe",
        },
        ToolAdapter {
            id: ToolId::Mux,
            display_name: "Mux",
            // add-skill global path: ~/.mux/skills/
            relative_skills_dir: ".mux/skills",
            relative_detect_dir: ".mux",
        },
        ToolAdapter {
            id: ToolId::OpenClaude,
            display_name: "OpenClaude IDE",
            // add-skill global path: ~/.openclaude/skills/
            relative_skills_dir: ".openclaude/skills",
            relative_detect_dir: ".openclaude",
        },
        ToolAdapter {
            id: ToolId::OpenHands,
            display_name: "OpenHands",
            // add-skill global path: ~/.openhands/skills/
            relative_skills_dir: ".openhands/skills",
            relative_detect_dir: ".openhands",
        },
        ToolAdapter {
            id: ToolId::Pi,
            display_name: "Pi",
            // add-skill global path: ~/.pi/agent/skills/
            relative_skills_dir: ".pi/agent/skills",
            relative_detect_dir: ".pi",
        },
        ToolAdapter {
            id: ToolId::Qoder,
            display_name: "Qoder",
            // add-skill global path: ~/.qoder/skills/
            relative_skills_dir: ".qoder/skills",
            relative_detect_dir: ".qoder",
        },
        ToolAdapter {
            id: ToolId::QoderWork,
            display_name: "QoderWork",
            // add-skill global path: ~/.qoderwork/skills/
            relative_skills_dir: ".qoderwork/skills",
            relative_detect_dir: ".qoderwork",
        },
        ToolAdapter {
            id: ToolId::QwenCode,
            display_name: "Qwen Code",
            // add-skill global path: ~/.qwen/skills/
            relative_skills_dir: ".qwen/skills",
            relative_detect_dir: ".qwen",
        },
        ToolAdapter {
            id: ToolId::Trae,
            display_name: "Trae",
            // add-skill global path: ~/.trae/skills/
            relative_skills_dir: ".trae/skills",
            relative_detect_dir: ".trae",
        },
        ToolAdapter {
            id: ToolId::TraeCn,
            display_name: "Trae CN",
            // add-skill global path: ~/.trae-cn/skills/
            relative_skills_dir: ".trae-cn/skills",
            relative_detect_dir: ".trae-cn",
        },
        ToolAdapter {
            id: ToolId::Zencoder,
            display_name: "Zencoder",
            // add-skill global path: ~/.zencoder/skills/
            relative_skills_dir: ".zencoder/skills",
            relative_detect_dir: ".zencoder",
        },
        ToolAdapter {
            id: ToolId::Neovate,
            display_name: "Neovate",
            // add-skill global path: ~/.neovate/skills/
            relative_skills_dir: ".neovate/skills",
            relative_detect_dir: ".neovate",
        },
        ToolAdapter {
            id: ToolId::Pochi,
            display_name: "Pochi",
            // add-skill global path: ~/.pochi/skills/
            relative_skills_dir: ".pochi/skills",
            relative_detect_dir: ".pochi",
        },
        ToolAdapter {
            id: ToolId::AdaL,
            display_name: "AdaL",
            // add-skill global path: ~/.adal/skills/
            relative_skills_dir: ".adal/skills",
            relative_detect_dir: ".adal",
        },
        ToolAdapter {
            id: ToolId::KiloCode,
            display_name: "Kilo Code",
            // add-skill global path: ~/.kilocode/skills/
            relative_skills_dir: ".kilocode/skills",
            relative_detect_dir: ".kilocode",
        },
        ToolAdapter {
            id: ToolId::RooCode,
            display_name: "Roo Code",
            // add-skill global path: ~/.roo/skills/
            relative_skills_dir: ".roo/skills",
            relative_detect_dir: ".roo",
        },
        ToolAdapter {
            id: ToolId::Goose,
            display_name: "Goose",
            // add-skill global path: ~/.config/goose/skills/
            relative_skills_dir: ".config/goose/skills",
            relative_detect_dir: ".config/goose",
        },
        ToolAdapter {
            id: ToolId::GeminiCli,
            display_name: "Gemini CLI",
            // add-skill global path: ~/.gemini/skills/
            relative_skills_dir: ".gemini/skills",
            relative_detect_dir: ".gemini",
        },
        ToolAdapter {
            id: ToolId::GithubCopilot,
            display_name: "GitHub Copilot",
            // add-skill global path: ~/.copilot/skills/
            relative_skills_dir: ".copilot/skills",
            relative_detect_dir: ".copilot",
        },
        ToolAdapter {
            id: ToolId::Clawdbot,
            display_name: "Clawdbot",
            // add-skill global path: ~/.clawdbot/skills/
            relative_skills_dir: ".clawdbot/skills",
            relative_detect_dir: ".clawdbot",
        },
        ToolAdapter {
            id: ToolId::Droid,
            display_name: "Droid",
            // add-skill global path: ~/.factory/skills/
            relative_skills_dir: ".factory/skills",
            relative_detect_dir: ".factory",
        },
        ToolAdapter {
            id: ToolId::Windsurf,
            display_name: "Windsurf",
            // add-skill global path: ~/.codeium/windsurf/skills/
            relative_skills_dir: ".codeium/windsurf/skills",
            relative_detect_dir: ".codeium/windsurf",
        },
        ToolAdapter {
            id: ToolId::Moltbot,
            display_name: "MoltBot",
            // add-skill global path: ~/.moltbot/skills/
            relative_skills_dir: ".moltbot/skills",
            relative_detect_dir: ".moltbot",
        },
        ToolAdapter {
            id: ToolId::HermesAgent,
            display_name: "Hermes Agent",
            // Hermes stores managed skills under HERMES_HOME/skills; default HERMES_HOME is ~/.hermes.
            relative_skills_dir: ".hermes/skills",
            relative_detect_dir: ".hermes",
        },
    ]
}

/// Tools can share the same global skills directory (e.g. Amp and Kimi Code CLI).
/// Use this to coordinate UI warnings and avoid duplicate filesystem operations.
pub fn adapters_sharing_skills_dir(adapter: &ToolAdapter) -> Vec<ToolAdapter> {
    default_tool_adapters()
        .into_iter()
        .filter(|a| a.relative_skills_dir == adapter.relative_skills_dir)
        .collect()
}

pub fn adapters_sharing_project_skills_dir(adapter: &ToolAdapter) -> Vec<ToolAdapter> {
    let relative = project_relative_skills_dir(adapter);
    default_tool_adapters()
        .into_iter()
        .filter(|a| project_relative_skills_dir(a) == relative)
        .collect()
}

pub fn adapter_by_key(key: &str) -> Option<ToolAdapter> {
    default_tool_adapters()
        .into_iter()
        .find(|adapter| adapter.id.as_key() == key)
}

pub fn resolve_default_path(adapter: &ToolAdapter) -> Result<PathBuf> {
    let home = dirs::home_dir().context("failed to resolve home directory")?;
    Ok(home.join(adapter.relative_skills_dir))
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn resolve_project_path(adapter: &ToolAdapter, project_root: &Path) -> Result<PathBuf> {
    Ok(project_root.join(project_relative_skills_dir(adapter)))
}

pub fn supports_project_scope(adapter: &ToolAdapter) -> bool {
    !matches!(adapter.id, ToolId::HermesAgent | ToolId::WorkBuddy)
}

pub fn project_relative_skills_dir(adapter: &ToolAdapter) -> &'static str {
    match adapter.id {
        ToolId::Amp | ToolId::KimiCli => ".agents/skills",
        ToolId::Antigravity => ".agents/skills",
        ToolId::Augment => ".augment/skills",
        ToolId::ClaudeCode => ".claude/skills",
        ToolId::OpenClaw => "skills",
        ToolId::Cline => ".agents/skills",
        ToolId::CodeBuddy => ".codebuddy/skills",
        ToolId::CodeWhale => ".codewhale/skills",
        ToolId::WorkBuddy => ".workbuddy/skills",
        ToolId::Codex => ".agents/skills",
        ToolId::DeepSeekHarness => ".dsh/skills",
        ToolId::CommandCode => ".commandcode/skills",
        ToolId::Continue => ".continue/skills",
        ToolId::Crush => ".crush/skills",
        ToolId::Cursor => ".agents/skills",
        ToolId::Droid => ".factory/skills",
        ToolId::GeminiCli => ".agents/skills",
        ToolId::GithubCopilot => ".agents/skills",
        ToolId::Goose => ".goose/skills",
        ToolId::Junie => ".junie/skills",
        ToolId::IflowCli => ".iflow/skills",
        ToolId::KiloCode => ".kilocode/skills",
        ToolId::KiroCli => ".kiro/skills",
        ToolId::Kode => ".kode/skills",
        ToolId::McpJam => ".mcpjam/skills",
        ToolId::MistralVibe => ".vibe/skills",
        ToolId::Mux => ".mux/skills",
        ToolId::OpenCode => ".agents/skills",
        ToolId::OpenHands => ".openhands/skills",
        ToolId::Pi => ".pi/skills",
        ToolId::Qoder => ".qoder/skills",
        ToolId::QwenCode => ".qwen/skills",
        ToolId::RooCode => ".roo/skills",
        ToolId::Trae | ToolId::TraeCn => ".trae/skills",
        ToolId::Windsurf => ".windsurf/skills",
        ToolId::Zencoder => ".zencoder/skills",
        ToolId::Neovate => ".neovate/skills",
        ToolId::Pochi => ".pochi/skills",
        ToolId::AdaL => ".adal/skills",
        ToolId::Copaw
        | ToolId::OpenClaude
        | ToolId::QoderWork
        | ToolId::Clawdbot
        | ToolId::Moltbot
        | ToolId::HermesAgent => adapter.relative_skills_dir,
    }
}

pub fn resolve_detect_path(adapter: &ToolAdapter) -> Result<PathBuf> {
    let home = dirs::home_dir().context("failed to resolve home directory")?;
    Ok(home.join(adapter.relative_detect_dir))
}

pub fn is_tool_installed(adapter: &ToolAdapter) -> Result<bool> {
    Ok(resolve_detect_path(adapter)?.exists())
}

pub fn scan_tool_dir(tool: &ToolAdapter, dir: &Path) -> Result<Vec<DetectedSkill>> {
    let mut results = Vec::new();
    if !dir.exists() {
        return Ok(results);
    }

    let ignore_hint = "Application Support/com.tauri.dev/skills";

    for entry in std::fs::read_dir(dir).with_context(|| format!("read dir {:?}", dir))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        let is_dir = file_type.is_dir() || (file_type.is_symlink() && path.is_dir());
        if !is_dir {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if tool.id == ToolId::Codex && name == ".system" {
            continue;
        }
        if tool.id == ToolId::Codex
            && path
                .canonicalize()
                .ok()
                .map(|target| {
                    target
                        .ancestors()
                        .any(|ancestor| ancestor.ends_with(Path::new(".codex/plugins/cache")))
                })
                .unwrap_or(false)
        {
            continue;
        }
        let has_skill_file = std::fs::read_dir(&path)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .any(|child| {
                child
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("SKILL.md")
                    && child.path().is_file()
            });
        if !has_skill_file {
            continue;
        }
        let (is_link, link_target) = detect_link(&path);
        if path.to_string_lossy().contains(ignore_hint)
            || link_target
                .as_ref()
                .map(|p| p.to_string_lossy().contains(ignore_hint))
                .unwrap_or(false)
        {
            continue;
        }
        results.push(DetectedSkill {
            tool: tool.id.clone(),
            name,
            path,
            is_link,
            link_target,
        });
    }

    Ok(results)
}

fn detect_link(path: &Path) -> (bool, Option<PathBuf>) {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let target = std::fs::read_link(path).ok();
            (true, target)
        }
        _ => {
            let target = std::fs::read_link(path).ok();
            if target.is_some() {
                (true, target)
            } else {
                (false, None)
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/tool_adapters.rs"]
mod tests;
