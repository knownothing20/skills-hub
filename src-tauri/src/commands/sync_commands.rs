use anyhow::Context;
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::core::central_repo::{
    ensure_central_repo, fixed_central_repo_path, resolve_central_repo_path,
};
use crate::core::content_hash::hash_dir;
use crate::core::safe_fs::{
    direct_skill_child, ensure_distinct_roots, lock_central_mutation, move_internal_to_trash,
    move_skill_to_trash, path_entry_exists, paths_have_same_identity,
    publish_staged_entry_no_replace, restore_skill_from_trash, validate_direct_skill_path,
    validate_skill_name, TrashReceipt,
};
use crate::core::skill_store::{SkillRecord, SkillStore, SkillTargetRecord};
use crate::core::sync_engine::{
    copy_dir_recursive, sync_dir_for_tool_with_overwrite, sync_dir_with_mode_with_overwrite,
    SyncMode, SyncOutcome,
};
use crate::core::tool_adapters::{
    adapter_by_key, adapters_sharing_project_skills_dir, is_tool_installed,
};

use super::common::{
    expand_home_path, format_anyhow_error, normalize_scope, now_ms, resolve_runtime_tool_root,
    runtime_tool_by_key, runtime_tools, runtime_tools_sharing_dir,
};

#[derive(Debug, Serialize)]
pub struct SyncResultDto {
    pub mode_used: String,
    pub target_path: String,
}

#[derive(Debug, Serialize)]
pub struct EnableSkillResultDto {
    pub restored_targets: usize,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SkillTargetComparisonDto {
    pub tool: String,
    pub tool_label: String,
    pub target_path: String,
    pub central_path: String,
    pub central_mtime: u64,
    pub tool_mtime: u64,
    pub central_file_count: usize,
    pub tool_file_count: usize,
    pub status: String,
    pub diff_description: String,
}

pub fn sync_mode_name(mode: SyncMode) -> &'static str {
    match mode {
        SyncMode::Auto => "auto",
        SyncMode::Symlink => "symlink",
        SyncMode::Junction => "junction",
        SyncMode::Copy => "copy",
    }
}

pub fn saved_sync_mode(mode: &str) -> anyhow::Result<SyncMode> {
    match mode {
        "auto" => Ok(SyncMode::Auto),
        "symlink" => Ok(SyncMode::Symlink),
        "junction" => Ok(SyncMode::Junction),
        "copy" => Ok(SyncMode::Copy),
        _ => anyhow::bail!("UNSAFE_PATH|Unknown saved sync mode {mode}"),
    }
}

pub fn target_has_same_content(source: &std::path::Path, target: &std::path::Path) -> bool {
    if !source.is_dir() || !target.is_dir() {
        return false;
    }
    match (hash_dir(source), hash_dir(target)) {
        (Ok(source_hash), Ok(target_hash)) => source_hash == target_hash,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn record_skill_target_failure(
    store: &SkillStore,
    skill_id: &str,
    tool: &str,
    scope: &str,
    project_path: Option<&str>,
    target_path: &std::path::Path,
    requested_mode: SyncMode,
    error: &str,
) -> anyhow::Result<()> {
    let existing = store.get_skill_target(skill_id, tool, scope, project_path)?;
    let record = SkillTargetRecord {
        id: existing
            .as_ref()
            .map(|target| target.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        skill_id: skill_id.to_string(),
        tool: tool.to_string(),
        scope: scope.to_string(),
        project_path: project_path.map(str::to_string),
        target_path: target_path.to_string_lossy().to_string(),
        mode: existing
            .as_ref()
            .map(|target| target.mode.clone())
            .unwrap_or_else(|| sync_mode_name(requested_mode).to_string()),
        status: "error".to_string(),
        last_error: Some(error.to_string()),
        synced_at: existing.and_then(|target| target.synced_at),
    };
    store.upsert_skill_target(&record)
}

pub fn auto_reconcile_skill_targets(
    store: &SkillStore,
    filter_skill_id: Option<&str>,
) -> anyhow::Result<usize> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Ok(0),
    };
    let adapters = crate::core::tool_adapters::default_tool_adapters();
    let skills = store.list_skills()?;
    let now = now_ms();
    let mut reconciled_count = 0;

    for skill in &skills {
        if let Some(fid) = filter_skill_id {
            if skill.id != fid {
                continue;
            }
        }
        let central_path = std::path::PathBuf::from(&skill.central_path);
        if !central_path.is_dir() {
            continue;
        }

        let central_ignore_path = central_path.join(".skillignore");
        let central_ignore_content = std::fs::read_to_string(&central_ignore_path).ok();

        for adapter in &adapters {
            // 快速短路检查：如果数据库中已经记录该目标且状态为 ok，直接 0 耗时跳过！
            let existing = store.get_skill_target(&skill.id, adapter.id.as_key(), "global", None)?;
            if let Some(ref rec) = existing {
                if rec.status == "ok" {
                    continue;
                }
            }

            let tool_dir = home.join(adapter.relative_skills_dir);
            if !tool_dir.is_dir() {
                continue;
            }
            let target_path = tool_dir.join(&skill.name);
            if !target_path.exists() {
                continue;
            }

            // 如果中心库有 .skillignore，而工具目录下还没有，同步一份规则过去以保持比对准则一致
            if let Some(ref ignore_str) = central_ignore_content {
                let target_ignore_path = target_path.join(".skillignore");
                if !target_ignore_path.exists() {
                    let _ = std::fs::write(&target_ignore_path, ignore_str);
                }
            }

            if target_has_same_content(&central_path, &target_path) {
                let mode = if target_path.is_symlink() {
                    "junction".to_string()
                } else {
                    "copy".to_string()
                };

                let record = SkillTargetRecord {
                    id: existing.map(|t| t.id).unwrap_or_else(|| Uuid::new_v4().to_string()),
                    skill_id: skill.id.clone(),
                    tool: adapter.id.as_key().to_string(),
                    scope: "global".to_string(),
                    project_path: None,
                    target_path: target_path.to_string_lossy().to_string(),
                    mode,
                    status: "ok".to_string(),
                    last_error: None,
                    synced_at: Some(now),
                };
                store.upsert_skill_target(&record)?;
                reconciled_count += 1;
            }
        }
    }

    Ok(reconciled_count)
}

#[tauri::command]
pub async fn sync_skill_dir(
    _source_path: String,
    _target_path: String,
) -> Result<SyncResultDto, String> {
    Err("SAFE_POLICY|External tool writes are disabled in Skills Hub".to_string())
}

#[allow(clippy::too_many_arguments)]
pub fn sync_skill_to_tool_impl(
    central_root: &std::path::Path,
    store: &SkillStore,
    source_path: &str,
    skill_id: &str,
    tool: &str,
    name: &str,
    overwrite: Option<bool>,
    overwrite_if_same_content: Option<bool>,
    scope: Option<&str>,
    project_path: Option<&str>,
    sync_mode: Option<&str>,
) -> anyhow::Result<SyncResultDto> {
    let _mutation_guard = lock_central_mutation()?;
    let skill = store
        .get_skill_by_id(skill_id)?
        .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
    validate_skill_name(&skill.name)?;
    validate_skill_name(name)?;
    if name != skill.name {
        anyhow::bail!("UNSAFE_PATH|Requested Skill name does not match managed record");
    }
    ensure_central_repo(central_root)?;
    let managed_source = std::path::PathBuf::from(&skill.central_path);
    validate_direct_skill_path(central_root, &managed_source)?;
    let expected_source = direct_skill_child(central_root, &skill.name)?;
    if !paths_have_same_identity(&managed_source, &expected_source)?
        || !paths_have_same_identity(std::path::Path::new(source_path), &managed_source)?
    {
        anyhow::bail!(
            "UNSAFE_PATH|sourcePath must exactly match the managed central Skill path"
        );
    }

    let runtime_tool = runtime_tool_by_key(store, tool)?;
    let scope = normalize_scope(scope)?;
    if scope == "project" && !runtime_tool.supports_project_scope {
        anyhow::bail!("PROJECT_SCOPE_UNSUPPORTED|{}", runtime_tool.key);
    }
    let project_root = if scope == "project" {
        let raw = project_path
            .ok_or_else(|| anyhow::anyhow!("projectPath is required for project scope"))?;
        let path = expand_home_path(raw)?;
        if !path.is_dir() {
            anyhow::bail!("projectPath must be an existing directory: {:?}", path);
        }
        Some(path)
    } else {
        None
    };

    let tool_root = resolve_runtime_tool_root(&runtime_tool, project_root.as_deref())?;
    std::fs::create_dir_all(&tool_root)
        .with_context(|| format!("create configured Skill root {:?}", tool_root))?;
    ensure_distinct_roots(&tool_root, central_root)?;
    let target = direct_skill_child(&tool_root, name)?;
    let project_path_for_record = project_root
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    if scope == "global" && !runtime_tool.installed {
        let error = format!("TOOL_NOT_INSTALLED|{}", runtime_tool.key);
        record_skill_target_failure(
            store,
            skill_id,
            tool,
            scope,
            project_path_for_record.as_deref(),
            &target,
            runtime_tool.sync_mode,
            &error,
        )?;
        anyhow::bail!(error);
    }
    // Pre-check: ensure the skills directory is writable (fixes #20 — Windows OS error 5).
    if let Err(err) = std::fs::create_dir_all(&tool_root) {
        let error = if err.kind() == std::io::ErrorKind::PermissionDenied {
            format!(
                "TOOL_NOT_WRITABLE|{}|{}",
                runtime_tool.label,
                tool_root.to_string_lossy()
            )
        } else {
            format!("failed to create skills dir {:?}: {}", tool_root, err)
        };
        record_skill_target_failure(
            store,
            skill_id,
            tool,
            scope,
            project_path_for_record.as_deref(),
            &target,
            runtime_tool.sync_mode,
            &error,
        )?;
        anyhow::bail!(error);
    }
    let default_mode = store
        .get_setting("default_sync_mode")
        .ok()
        .flatten()
        .unwrap_or_else(|| "copy".to_string());
    let effective_mode_str = sync_mode.unwrap_or(&default_mode);
    let requested_mode = match effective_mode_str {
        "copy" => Some(crate::core::sync_engine::SyncMode::Copy),
        "junction" => Some(crate::core::sync_engine::SyncMode::Junction),
        "symlink" => Some(crate::core::sync_engine::SyncMode::Symlink),
        _ => None,
    };

    if let Some(existing) =
        store.get_skill_target(skill_id, tool, scope, project_path_for_record.as_deref())?
    {
        let matches_requested_mode = match requested_mode {
            Some(crate::core::sync_engine::SyncMode::Copy) => existing.mode == "copy",
            Some(crate::core::sync_engine::SyncMode::Junction)
            | Some(crate::core::sync_engine::SyncMode::Symlink) => {
                existing.mode == "junction" || existing.mode == "symlink"
            }
            _ => true,
        };
        if existing.status == "ok"
            && existing.target_path == target.to_string_lossy()
            && target.exists()
            && matches_requested_mode
            && overwrite != Some(true)
        {
            return Ok(SyncResultDto {
                mode_used: existing.mode,
                target_path: existing.target_path,
            });
        }
    }
    let is_same_content = target_has_same_content(&managed_source, &target);
    let is_source_path = skill
        .source_ref
        .as_deref()
        .map(|r| paths_have_same_identity(std::path::Path::new(r), &target).unwrap_or(false))
        .unwrap_or(false);
    let can_adopt_existing = is_same_content || is_source_path;

    // 如果软件目录中已经存在同内容的真实实体文件夹（非软链接）：
    // 1. 如果要求的是 Copy（或默认 Copy）；
    // 2. 或者该目录本身就是技能被发现导入时的原始来源物理路径（is_source_path）：
    // 绝不将原实体替换为软链接，直接保持软件原有的实体物理目录，登记为实体副本。
    let is_target_physical_dir = target.is_dir() && !target.is_symlink();
    let is_copy_mode = match requested_mode {
        Some(crate::core::sync_engine::SyncMode::Copy) => true,
        None => default_mode == "copy",
        _ => false,
    };
    let should_keep_entity_as_copy = can_adopt_existing
        && is_target_physical_dir
        && (is_copy_mode || is_source_path);

    let result = if should_keep_entity_as_copy {
        log::info!(
            "target directory {:?} is already a real physical directory with matching content, keeping original entity as Copy",
            target
        );
        SyncOutcome {
            mode_used: SyncMode::Copy,
            target_path: target.clone(),
            replaced: false,
        }
    } else {
        let overwrite = overwrite.unwrap_or(false)
            || (overwrite_if_same_content.unwrap_or(false) && is_same_content)
            || is_source_path
            || requested_mode.is_some();
        let sync_res = if let Some(m) = requested_mode {
            sync_dir_with_mode_with_overwrite(m, &managed_source, &target, overwrite)
        } else if runtime_tool.is_custom {
            sync_dir_with_mode_with_overwrite(
                runtime_tool.sync_mode,
                &managed_source,
                &target,
                overwrite,
            )
        } else {
            sync_dir_for_tool_with_overwrite(tool, &managed_source, &target, overwrite)
        };
        match sync_res {
            Ok(result) => result,
            Err(err) => {
                if can_adopt_existing && target.is_dir() {
                    log::info!(
                        "target directory {:?} matches source/content, adopting existing directory: {}",
                        target,
                        err
                    );
                    SyncOutcome {
                        mode_used: SyncMode::Copy,
                        target_path: target.clone(),
                        replaced: false,
                    }
                } else {
                    let msg = err.to_string();
                    let error = if msg.contains("target already exists") {
                        format!("TARGET_EXISTS|{}", target.to_string_lossy())
                    } else if msg.contains("os error 5")
                        || msg.contains("Access is denied")
                        || msg.contains("Permission denied")
                    {
                        format!(
                            "TOOL_NOT_WRITABLE|{}|{}",
                            runtime_tool.label,
                            tool_root.to_string_lossy()
                        )
                    } else {
                        msg
                    };
                    record_skill_target_failure(
                        store,
                        skill_id,
                        tool,
                        scope,
                        project_path_for_record.as_deref(),
                        &target,
                        runtime_tool.sync_mode,
                        &error,
                    )?;
                    anyhow::bail!(error);
                }
            }
        }
    };

    // Some tools share the same skills directory; keep DB records consistent across them.
    let group = runtime_tools_sharing_dir(store, &runtime_tool, scope)?;
    for a in group {
        let record = SkillTargetRecord {
            id: Uuid::new_v4().to_string(),
            skill_id: skill_id.to_string(),
            tool: a.key,
            scope: scope.to_string(),
            project_path: project_path_for_record.clone(),
            target_path: result.target_path.to_string_lossy().to_string(),
            mode: match result.mode_used {
                SyncMode::Auto => "auto",
                SyncMode::Symlink => "symlink",
                SyncMode::Junction => "junction",
                SyncMode::Copy => "copy",
            }
            .to_string(),
            status: "ok".to_string(),
            last_error: None,
            synced_at: Some(now_ms()),
        };
        store.upsert_skill_target(&record)?;
    }

    Ok(SyncResultDto {
        mode_used: match result.mode_used {
            SyncMode::Auto => "auto",
            SyncMode::Symlink => "symlink",
            SyncMode::Junction => "junction",
            SyncMode::Copy => "copy",
        }
        .to_string(),
        target_path: result.target_path.to_string_lossy().to_string(),
    })
}

#[tauri::command]
#[allow(non_snake_case)]
#[allow(clippy::too_many_arguments)]
pub async fn sync_skill_to_tool(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    sourcePath: String,
    skillId: String,
    tool: String,
    name: String,
    overwrite: Option<bool>,
    overwriteIfSameContent: Option<bool>,
    scope: Option<String>,
    projectPath: Option<String>,
    syncMode: Option<String>,
) -> Result<SyncResultDto, String> {
    let store = store.inner().clone();
    let central_root = resolve_central_repo_path(&app, &store)
        .map_err(|err| err.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        sync_skill_to_tool_impl(
            &central_root,
            &store,
            &sourcePath,
            &skillId,
            &tool,
            &name,
            overwrite,
            overwriteIfSameContent,
            scope.as_deref(),
            projectPath.as_deref(),
            syncMode.as_deref(),
        )
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

pub fn validated_managed_target(
    store: &SkillStore,
    target: &SkillTargetRecord,
    expected_name: &str,
) -> anyhow::Result<(std::path::PathBuf, std::path::PathBuf)> {
    validate_skill_name(expected_name)?;
    let runtime_tool = runtime_tools(store, true)?
        .into_iter()
        .find(|candidate| candidate.key == target.tool)
        .ok_or_else(|| anyhow::anyhow!("UNSAFE_PATH|Unknown configured tool {}", target.tool))?;
    let project_root = if target.scope == "project" {
        let raw = target
            .project_path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("UNSAFE_PATH|Project target has no project root"))?;
        let root = expand_home_path(raw)?;
        if !root.is_dir() {
            anyhow::bail!(
                "UNSAFE_PATH|Project target root no longer exists: {:?}",
                root
            );
        }
        Some(root)
    } else if target.scope == "global" {
        None
    } else {
        anyhow::bail!("UNSAFE_PATH|Unknown target scope {}", target.scope);
    };
    let root = resolve_runtime_tool_root(&runtime_tool, project_root.as_deref())?;
    let fixed_central = fixed_central_repo_path()?;
    if fixed_central.is_dir() {
        ensure_distinct_roots(&root, &fixed_central)?;
    }
    let path = std::path::PathBuf::from(&target.target_path);
    let actual_name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| anyhow::anyhow!("UNSAFE_PATH|Managed target has no UTF-8 name"))?;
    if actual_name != expected_name {
        anyhow::bail!("UNSAFE_PATH|Managed target name does not match the Skill record");
    }
    validate_direct_skill_path(&root, &path)?;
    Ok((root, path))
}

#[derive(Debug)]
pub struct TrashedManagedPath {
    pub root: std::path::PathBuf,
    pub original: std::path::PathBuf,
    pub trash_receipt: TrashReceipt,
}

pub fn move_managed_target_to_trash(
    store: &SkillStore,
    target: &SkillTargetRecord,
    expected_name: &str,
) -> anyhow::Result<Option<TrashedManagedPath>> {
    let (root, path) = validated_managed_target(store, target, expected_name)?;
    Ok(
        move_skill_to_trash(&root, &path)?.map(|trash_receipt| TrashedManagedPath {
            root,
            original: path,
            trash_receipt,
        }),
    )
}

pub fn rollback_trashed_managed_paths(moved: &[TrashedManagedPath]) -> anyhow::Result<()> {
    let mut errors = Vec::new();
    for item in moved.iter().rev() {
        if let Err(err) = restore_skill_from_trash(&item.root, &item.original, &item.trash_receipt)
        {
            errors.push(format!("{}: {err:#}", item.original.display()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("ROLLBACK_INCOMPLETE|{}", errors.join(" | "))
    }
}

pub fn move_managed_targets_to_trash(
    store: &SkillStore,
    targets: &[SkillTargetRecord],
    expected_name: &str,
) -> anyhow::Result<Vec<TrashedManagedPath>> {
    let mut moved = Vec::new();
    let mut moved_paths = std::collections::HashSet::new();
    for target in targets {
        if !moved_paths.insert(target.target_path.clone()) {
            continue;
        }
        match move_managed_target_to_trash(store, target, expected_name) {
            Ok(Some(item)) => moved.push(item),
            Ok(None) => {}
            Err(err) => {
                rollback_trashed_managed_paths(&moved).with_context(|| {
                    format!("target move failed ({err:#}) and rollback was incomplete")
                })?;
                return Err(err).context("target move failed; earlier moves were restored");
            }
        }
    }
    Ok(moved)
}

#[derive(Clone, Debug)]
pub struct EnableTargetPlan {
    pub root: std::path::PathBuf,
    pub path: std::path::PathBuf,
    pub mode: SyncMode,
}

#[cfg(unix)]
#[derive(Debug, Eq, PartialEq)]
pub struct EnablePathIdentity {
    pub device: u64,
    pub inode: u64,
    pub mode: u32,
}

#[cfg(windows)]
#[derive(Debug, Eq, PartialEq)]
pub struct EnablePathIdentity {
    pub handle: same_file::Handle,
    pub file_attributes: u32,
    pub created_at: u64,
}

#[cfg(not(any(unix, windows)))]
#[derive(Debug, Eq, PartialEq)]
pub struct EnablePathIdentity;

#[derive(Debug)]
pub struct OwnedEnableTarget {
    pub root: std::path::PathBuf,
    pub path: std::path::PathBuf,
    pub identity: EnablePathIdentity,
    pub mode_used: SyncMode,
}

pub fn preflight_enable_targets<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
    skill: &SkillRecord,
) -> anyhow::Result<(Vec<SkillTargetRecord>, Vec<EnableTargetPlan>)> {
    validate_skill_name(&skill.name)?;
    let central_root = resolve_central_repo_path(app, store)?;
    let managed_source = std::path::PathBuf::from(&skill.central_path);
    validate_direct_skill_path(&central_root, &managed_source)?;
    let expected_source = direct_skill_child(&central_root, &skill.name)?;
    let metadata = std::fs::symlink_metadata(&managed_source)
        .with_context(|| format!("stat managed central Skill {:?}", managed_source))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        anyhow::bail!("UNSAFE_PATH|Managed central Skill must be a real directory");
    }
    if !paths_have_same_identity(&managed_source, &expected_source)? {
        anyhow::bail!("UNSAFE_PATH|Managed central Skill path does not match its record");
    }

    let original_targets = store.list_skill_targets(&skill.id)?;
    let mut plans_by_path =
        std::collections::BTreeMap::<std::path::PathBuf, EnableTargetPlan>::new();

    // Validate every saved row before creating even the first target.
    for target in &original_targets {
        if target.status != "disabled" {
            anyhow::bail!(
                "INCONSISTENT_TARGET_STATE|Saved target {} is not disabled",
                target.tool
            );
        }
        let runtime_tool = runtime_tool_by_key(store, &target.tool)?;
        if target.scope == "global" && !runtime_tool.installed {
            anyhow::bail!("TOOL_NOT_INSTALLED|{}", runtime_tool.key);
        }
        if target.scope == "project" && !runtime_tool.supports_project_scope {
            anyhow::bail!("PROJECT_SCOPE_UNSUPPORTED|{}", runtime_tool.key);
        }

        let (root, path) = validated_managed_target(store, target, &skill.name)?;
        ensure_distinct_roots(&root, &central_root)?;
        if path_entry_exists(&path)? {
            anyhow::bail!("TARGET_EXISTS|{}", path.to_string_lossy());
        }
        let mode = saved_sync_mode(&target.mode)?;

        if let Some(existing) = plans_by_path.get_mut(&path) {
            if existing.root != root || existing.mode != mode {
                anyhow::bail!(
                    "INCONSISTENT_TARGET_STATE|Shared target rows disagree on root or sync mode"
                );
            }
        } else {
            plans_by_path.insert(path.clone(), EnableTargetPlan { root, path, mode });
        }
    }

    Ok((original_targets, plans_by_path.into_values().collect()))
}

#[cfg(unix)]
pub fn capture_enable_path_identity(path: &std::path::Path) -> anyhow::Result<EnablePathIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("stat operation-owned target {:?}", path))?;
    Ok(EnablePathIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
    })
}

#[cfg(windows)]
pub fn capture_enable_path_identity(path: &std::path::Path) -> anyhow::Result<EnablePathIdentity> {
    use std::os::windows::fs::MetadataExt;

    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("stat operation-owned target {:?}", path))?;
    let handle = same_file::Handle::from_path(path)
        .with_context(|| format!("open operation-owned target {:?}", path))?;
    Ok(EnablePathIdentity {
        handle,
        file_attributes: metadata.file_attributes(),
        created_at: metadata.creation_time(),
    })
}

#[cfg(not(any(unix, windows)))]
pub fn capture_enable_path_identity(_path: &std::path::Path) -> anyhow::Result<EnablePathIdentity> {
    anyhow::bail!("UNSUPPORTED_PLATFORM|Filesystem identity is unavailable")
}

pub fn enable_path_identity_matches(
    path: &std::path::Path,
    expected: &EnablePathIdentity,
) -> anyhow::Result<bool> {
    if !path_entry_exists(path)? {
        return Ok(false);
    }
    Ok(capture_enable_path_identity(path)? == *expected)
}

pub fn move_owned_enable_target_to_trash(
    target: &OwnedEnableTarget,
    internal: bool,
) -> anyhow::Result<()> {
    if !path_entry_exists(&target.path)? {
        return Ok(());
    }
    if !enable_path_identity_matches(&target.path, &target.identity)? {
        anyhow::bail!(
            "OWNERSHIP_CHANGED|Refusing to move a replaced target: {}",
            target.path.display()
        );
    }
    if internal {
        move_internal_to_trash(&target.root, &target.path)?;
    } else {
        move_skill_to_trash(&target.root, &target.path)?;
    }
    Ok(())
}

pub fn rollback_restored_enable_targets(targets: &[OwnedEnableTarget]) -> anyhow::Result<()> {
    let mut errors = Vec::new();
    for target in targets.iter().rev() {
        if let Err(err) = move_owned_enable_target_to_trash(target, false) {
            errors.push(format!("{}: {err:#}", target.path.display()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("ROLLBACK_INCOMPLETE|{}", errors.join(" | "))
    }
}

pub fn unique_enable_staging_path(root: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    for _ in 0..8 {
        let candidate = root.join(format!(".skills-hub-enable-{}", Uuid::new_v4().simple()));
        if !path_entry_exists(&candidate)? {
            return Ok(candidate);
        }
    }
    anyhow::bail!("STAGING_COLLISION|Could not allocate an enable staging path")
}

pub fn stage_enable_copy(
    source: &std::path::Path,
    root: &std::path::Path,
) -> anyhow::Result<OwnedEnableTarget> {
    for _ in 0..8 {
        let staged = unique_enable_staging_path(root)?;
        match std::fs::create_dir(&staged) {
            Ok(()) => {
                let identity = capture_enable_path_identity(&staged)?;
                let owned = OwnedEnableTarget {
                    root: root.to_path_buf(),
                    path: staged,
                    identity,
                    mode_used: SyncMode::Copy,
                };
                if let Err(copy_err) = copy_dir_recursive(source, &owned.path) {
                    if let Err(rollback_err) = move_owned_enable_target_to_trash(&owned, true) {
                        anyhow::bail!(
                            "ROLLBACK_INCOMPLETE|enable staging copy failed ({copy_err:#}); {rollback_err:#}"
                        );
                    }
                    return Err(copy_err)
                        .context("enable staging copy failed; staging was moved to Trash");
                }
                return Ok(owned);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(anyhow::Error::new(err))
                    .with_context(|| format!("create enable staging directory {:?}", staged));
            }
        }
    }
    anyhow::bail!("STAGING_COLLISION|Could not claim an enable staging path")
}

#[cfg(unix)]
pub fn create_enable_staged_symlink(
    source: &std::path::Path,
    staged: &std::path::Path,
) -> anyhow::Result<()> {
    std::os::unix::fs::symlink(source, staged)
        .with_context(|| format!("create enable staging symlink {:?}", staged))
}

#[cfg(windows)]
pub fn create_enable_staged_symlink(
    source: &std::path::Path,
    staged: &std::path::Path,
) -> anyhow::Result<()> {
    std::os::windows::fs::symlink_dir(source, staged)
        .with_context(|| format!("create enable staging symlink {:?}", staged))
}

#[cfg(not(any(unix, windows)))]
pub fn create_enable_staged_symlink(
    _source: &std::path::Path,
    _staged: &std::path::Path,
) -> anyhow::Result<()> {
    anyhow::bail!("UNSUPPORTED_PLATFORM|Directory symlinks are unavailable")
}

#[cfg(windows)]
pub fn create_enable_staged_junction(
    source: &std::path::Path,
    staged: &std::path::Path,
) -> anyhow::Result<()> {
    junction::create(source, staged)
        .with_context(|| format!("create enable staging junction {:?}", staged))
}

#[cfg(not(windows))]
pub fn create_enable_staged_junction(
    _source: &std::path::Path,
    _staged: &std::path::Path,
) -> anyhow::Result<()> {
    anyhow::bail!("UNSUPPORTED_PLATFORM|Directory junctions are only available on Windows")
}

pub fn stage_enable_target(
    source: &std::path::Path,
    plan: &EnableTargetPlan,
) -> anyhow::Result<OwnedEnableTarget> {
    if matches!(plan.mode, SyncMode::Auto | SyncMode::Copy) {
        // Auto deliberately selects its safe Copy fallback here. The staged
        // directory is claimed before copying, so a competing process can
        // neither receive our files nor be mistaken for our partial output.
        return stage_enable_copy(source, &plan.root);
    }

    let staged = unique_enable_staging_path(&plan.root)?;
    match plan.mode {
        SyncMode::Symlink => create_enable_staged_symlink(source, &staged)?,
        SyncMode::Junction => create_enable_staged_junction(source, &staged)?,
        SyncMode::Auto | SyncMode::Copy => unreachable!("copy modes returned above"),
    }
    Ok(OwnedEnableTarget {
        root: plan.root.clone(),
        path: staged.clone(),
        identity: capture_enable_path_identity(&staged)?,
        mode_used: plan.mode,
    })
}

pub fn restore_enable_target_staged_with<B>(
    source: &std::path::Path,
    plan: &EnableTargetPlan,
    before_publish: B,
) -> anyhow::Result<OwnedEnableTarget>
where
    B: FnOnce(&EnableTargetPlan, &std::path::Path) -> anyhow::Result<()>,
{
    let mut staged = stage_enable_target(source, plan)?;
    if let Err(hook_err) = before_publish(plan, &staged.path) {
        if let Err(rollback_err) = move_owned_enable_target_to_trash(&staged, true) {
            anyhow::bail!(
                "ROLLBACK_INCOMPLETE|enable pre-publish step failed ({hook_err:#}); {rollback_err:#}"
            );
        }
        return Err(hook_err).context("enable pre-publish step failed; staging was rolled back");
    }
    if let Err(publish_err) = publish_staged_entry_no_replace(&plan.root, &staged.path, &plan.path)
    {
        if let Err(rollback_err) = move_owned_enable_target_to_trash(&staged, true) {
            anyhow::bail!(
                "ROLLBACK_INCOMPLETE|enable publish failed ({publish_err:#}); {rollback_err:#}"
            );
        }
        return Err(publish_err)
            .context("enable publish failed; operation-owned staging was rolled back");
    }

    staged.path = plan.path.clone();
    staged.root = plan.root.clone();
    Ok(staged)
}

pub fn enable_skill_and_restore_targets_with<R, F>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
    skill_id: &str,
    mut restore_target: F,
) -> anyhow::Result<EnableSkillResultDto>
where
    R: tauri::Runtime,
    F: FnMut(&std::path::Path, &EnableTargetPlan) -> anyhow::Result<OwnedEnableTarget>,
{
    let _mutation_guard = lock_central_mutation()?;
    let skill = store
        .get_skill_by_id(skill_id)?
        .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
    if skill.enabled {
        return Ok(EnableSkillResultDto {
            restored_targets: 0,
        });
    }

    let (original_targets, plans) = preflight_enable_targets(app, store, &skill)?;
    let managed_source = std::path::PathBuf::from(&skill.central_path);
    let mut restored_journal = Vec::new();
    let mut mode_by_path = std::collections::BTreeMap::new();

    for plan in &plans {
        let restored = match restore_target(&managed_source, plan) {
            Ok(restored) => {
                let ownership_check = if restored.root == plan.root && restored.path == plan.path {
                    enable_path_identity_matches(&restored.path, &restored.identity)
                } else {
                    Ok(false)
                };
                match ownership_check {
                    Ok(true) => restored,
                    Ok(false) => {
                        rollback_restored_enable_targets(&restored_journal)?;
                        anyhow::bail!(
                            "OWNERSHIP_CHANGED|Restore operation returned an unowned or replaced target"
                        );
                    }
                    Err(identity_err) => {
                        if let Err(rollback_err) =
                            rollback_restored_enable_targets(&restored_journal)
                        {
                            anyhow::bail!(
                                "ROLLBACK_INCOMPLETE|target identity check failed ({identity_err:#}); {rollback_err:#}"
                            );
                        }
                        return Err(identity_err).context(
                            "enable target identity check failed; earlier targets were rolled back",
                        );
                    }
                }
            }
            Err(restore_err) => {
                // The failed current plan is intentionally absent from the
                // journal. Only a successful restore can return an identity
                // proving that its live path belongs to this operation.
                if let Err(rollback_err) = rollback_restored_enable_targets(&restored_journal) {
                    anyhow::bail!(
                        "ROLLBACK_INCOMPLETE|target restore failed ({restore_err:#}); {rollback_err:#}"
                    );
                }
                return Err(restore_err)
                    .context("enable target restore failed; restored targets were rolled back");
            }
        };
        mode_by_path.insert(
            plan.path.clone(),
            sync_mode_name(restored.mode_used).to_string(),
        );
        restored_journal.push(restored);
    }

    let synced_at = now_ms();
    let restored_records_result = original_targets
        .iter()
        .map(|target| {
            let mut restored = target.clone();
            restored.mode = mode_by_path
                .get(std::path::Path::new(&target.target_path))
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("INCONSISTENT_TARGET_STATE|Missing restored target outcome")
                })?;
            restored.status = "ok".to_string();
            restored.last_error = None;
            restored.synced_at = Some(synced_at);
            Ok(restored)
        })
        .collect::<anyhow::Result<Vec<_>>>();
    let restored_records = match restored_records_result {
        Ok(targets) => targets,
        Err(state_err) => {
            if let Err(rollback_err) = rollback_restored_enable_targets(&restored_journal) {
                anyhow::bail!(
                    "ROLLBACK_INCOMPLETE|enable state preparation failed ({state_err:#}); {rollback_err:#}"
                );
            }
            return Err(state_err)
                .context("enable state preparation failed; restored targets were rolled back");
        }
    };

    if let Err(db_err) = store.enable_skill_with_targets_atomically(skill_id, &restored_records) {
        if let Err(rollback_err) = rollback_restored_enable_targets(&restored_journal) {
            anyhow::bail!(
                "ROLLBACK_INCOMPLETE|enable DB update failed ({db_err:#}); {rollback_err:#}"
            );
        }
        return Err(db_err)
            .context("enable DB transaction failed; database rolled back and files restored");
    }

    Ok(EnableSkillResultDto {
        restored_targets: original_targets.len(),
    })
}

pub fn enable_skill_and_restore_targets_impl<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
    skill_id: &str,
) -> anyhow::Result<EnableSkillResultDto> {
    enable_skill_and_restore_targets_with(app, store, skill_id, |source, plan| {
        restore_enable_target_staged_with(source, plan, |_plan, _staged| Ok(()))
    })
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn enable_skill_and_restore_targets(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: String,
) -> Result<EnableSkillResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        enable_skill_and_restore_targets_impl(&app, &store, &skillId)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn unsync_skill_from_tool(
    store: State<'_, SkillStore>,
    skillId: String,
    tool: String,
    scope: Option<String>,
    projectPath: Option<String>,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _mutation_guard = lock_central_mutation()?;
        let skill = store
            .get_skill_by_id(&skillId)?
            .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
        validate_skill_name(&skill.name)?;
        let scope = normalize_scope(scope.as_deref())?;
        let project_path = if scope == "project" {
            let raw = projectPath
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("projectPath is required for project scope"))?;
            Some(expand_home_path(raw)?.to_string_lossy().to_string())
        } else {
            None
        };

        // Some tools share the same skills directory; unsync should update all of them.
        let group_tool_keys: Vec<String> =
            if let Ok(runtime_tool) = runtime_tool_by_key(&store, &tool) {
                runtime_tools_sharing_dir(&store, &runtime_tool, scope)?
                    .into_iter()
                    .map(|tool| tool.key)
                    .collect()
            } else if let Some(adapter) = adapter_by_key(&tool) {
                let group = if scope == "project" {
                    adapters_sharing_project_skills_dir(&adapter)
                } else {
                    crate::core::tool_adapters::adapters_sharing_skills_dir(&adapter)
                };
                // If none of the group tools are installed, do nothing (treat as already not effective).
                if scope == "global" {
                    let mut any_installed = false;
                    for a in &group {
                        if is_tool_installed(a)? {
                            any_installed = true;
                            break;
                        }
                    }
                    if !any_installed {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                group
                    .into_iter()
                    .map(|a| a.id.as_key().to_string())
                    .collect()
            } else {
                vec![tool.clone()]
            };

        // Validate every stored path before mutating any path or DB record.
        let fixed_central = fixed_central_repo_path().ok();
        let mut selected_targets = Vec::new();
        let mut central_coinciding_targets = Vec::new();
        for k in &group_tool_keys {
            if let Some(target) =
                store.get_skill_target(&skillId, k, scope, project_path.as_deref())?
            {
                let is_central_target = if let Some(ref central) = fixed_central {
                    let p = std::path::Path::new(&target.target_path);
                    p == std::path::Path::new(&skill.central_path)
                        || (p.starts_with(central) && target.scope == "global")
                } else {
                    false
                };

                if is_central_target {
                    central_coinciding_targets.push(target);
                } else {
                    validated_managed_target(&store, &target, &skill.name)?;
                    selected_targets.push(target);
                }
            }
        }

        // 清理与中心母版重合的虚假 target 记录（不执行物理文件删除）
        for target in &central_coinciding_targets {
            let _ = store.delete_skill_target(
                &skillId,
                &target.tool,
                &target.scope,
                target.project_path.as_deref(),
            );
        }

        let moved = move_managed_targets_to_trash(&store, &selected_targets, &skill.name)?;
        for target in &selected_targets {
            if let Err(db_err) = store.delete_skill_target(
                &skillId,
                &target.tool,
                &target.scope,
                target.project_path.as_deref(),
            ) {
                let mut recovery_errors = Vec::new();
                for original in &selected_targets {
                    if let Err(err) = store.upsert_skill_target(original) {
                        recovery_errors.push(format!("restore DB target: {err:#}"));
                    }
                }
                if let Err(err) = rollback_trashed_managed_paths(&moved) {
                    recovery_errors.push(format!("restore files: {err:#}"));
                }
                if recovery_errors.is_empty() {
                    return Err(db_err)
                        .context("unsync DB update failed; files and records restored");
                }
                anyhow::bail!(
                    "ROLLBACK_INCOMPLETE|unsync DB update failed ({db_err:#}); {}",
                    recovery_errors.join(" | ")
                );
            }
        }

        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn reconcile_all_skill_targets(
    store: State<'_, SkillStore>,
) -> Result<usize, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        auto_reconcile_skill_targets(&store, None).map_err(format_anyhow_error)
    })
    .await
    .map_err(|err| err.to_string())?
}

pub fn scan_dir_latest_mtime(dir: &std::path::Path) -> (u64, usize) {
    if !dir.is_dir() {
        return (0, 0);
    }
    let mut max_mtime = 0u64;
    let mut file_count = 0usize;
    let rules = crate::core::content_hash::load_ignore_patterns(dir);
    let walker = walkdir::WalkDir::new(dir).follow_links(false).into_iter();
    for entry in walker.filter_entry(|e| !crate::core::content_hash::is_entry_ignored(dir, e, &rules)) {
        if let Ok(entry) = entry {
            if entry.file_type().is_file() {
                file_count += 1;
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        let ms = modified
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        if ms > max_mtime {
                            max_mtime = ms;
                        }
                    }
                }
            }
        }
    }
    (max_mtime, file_count)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn get_skill_target_comparisons(
    store: State<'_, SkillStore>,
    skillId: String,
) -> Result<Vec<SkillTargetComparisonDto>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skillId)?
            .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
        let central_path = std::path::PathBuf::from(&skill.central_path);
        let (central_mtime, central_files) = scan_dir_latest_mtime(&central_path);

        let targets = store.list_skill_targets(&skillId)?;
        let mut results = Vec::new();

        for target in targets {
            let target_path = std::path::PathBuf::from(&target.target_path);
            if !target_path.exists() {
                results.push(SkillTargetComparisonDto {
                    tool: target.tool.clone(),
                    tool_label: target.tool.clone(),
                    target_path: target.target_path.clone(),
                    central_path: skill.central_path.clone(),
                    central_mtime,
                    tool_mtime: 0,
                    central_file_count: central_files,
                    tool_file_count: 0,
                    status: "missing".to_string(),
                    diff_description: "目标目录不存在".to_string(),
                });
                continue;
            }

            let (tool_mtime, tool_files) = scan_dir_latest_mtime(&target_path);

            let status = if tool_mtime > central_mtime + 2000 {
                "tool_newer".to_string()
            } else if central_mtime > tool_mtime + 2000 {
                "central_newer".to_string()
            } else {
                "synced".to_string()
            };

            let diff_description = match status.as_str() {
                "tool_newer" => "检测到此工具修改较新，可设为母版",
                "central_newer" => "母版较新，可更新此软件",
                _ => "内容与母版完全一致",
            }
            .to_string();

            results.push(SkillTargetComparisonDto {
                tool: target.tool.clone(),
                tool_label: target.tool.clone(),
                target_path: target.target_path.clone(),
                central_path: skill.central_path.clone(),
                central_mtime,
                tool_mtime,
                central_file_count: central_files,
                tool_file_count: tool_files,
                status,
                diff_description,
            });
        }

        Ok::<_, anyhow::Error>(results)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn promote_target_to_central(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: String,
    tool: String,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock_central_mutation()?;
        let skill = store
            .get_skill_by_id(&skillId)?
            .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
        let target = store
            .get_skill_target(&skillId, &tool, "global", None)?
            .ok_or_else(|| anyhow::anyhow!("target tool not found for skill"))?;

        let src_dir = std::path::PathBuf::from(&target.target_path);
        let dst_dir = std::path::PathBuf::from(&skill.central_path);

        if !src_dir.is_dir() {
            anyhow::bail!("Source tool directory {:?} does not exist", src_dir);
        }

        let ignore_rules = crate::core::content_hash::load_ignore_patterns(&dst_dir);

        // 递归反向拷贝文件：目标软件 -> 中心母版 (跳过受保护的忽略规则，如 local/)
        for entry in walkdir::WalkDir::new(&src_dir).into_iter().filter_map(|e| e.ok()) {
            if crate::core::content_hash::is_entry_ignored(&src_dir, &entry, &ignore_rules) {
                continue;
            }

            let rel_path = match entry.path().strip_prefix(&src_dir) {
                Ok(p) => p,
                Err(_) => continue,
            };

            let dst_file = dst_dir.join(rel_path);
            if entry.file_type().is_dir() {
                let _ = std::fs::create_dir_all(&dst_file);
            } else if entry.file_type().is_file() {
                if let Some(parent) = dst_file.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::copy(entry.path(), &dst_file);
            }
        }

        // 重新计算母版 content_hash 并更新 SQLite 记录
        if let Ok(new_hash) = crate::core::content_hash::hash_dir(&dst_dir) {
            if let Ok(Some(mut record)) = store.get_skill_by_id(&skillId) {
                record.content_hash = Some(new_hash);
                record.updated_at = now_ms();
                let _ = store.upsert_skill(&record);
            }
        }

        let _ = auto_reconcile_skill_targets(&store, Some(&skillId));

        // 设为母版成功后，若开启了自动备份，异步触发一次后台增量备份
        let app_clone = app.clone();
        let store_clone = store.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let config = crate::core::backup::get_backup_config(&store_clone);
            if config.enabled {
                let _ = crate::core::backup::perform_backup(&app_clone, &store_clone, None);
            }
        });

        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}
