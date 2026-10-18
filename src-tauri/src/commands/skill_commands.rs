use anyhow::Context;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::core::auto_update::{managed_skill_update_capability, ManagedSkillUpdateCapability};
use crate::core::central_repo::{ensure_central_repo, resolve_central_repo_path};
use crate::core::installer::update_managed_skill_from_source;
use crate::core::safe_fs::{
    direct_skill_child, lock_central_mutation, move_skill_to_trash, paths_have_same_identity,
    restore_skill_from_trash, validate_direct_skill_path, validate_skill_name,
};
use crate::core::skill_metadata::read_skill_ui_metadata;
use crate::core::skill_store::{SkillRecord, SkillStore};

use super::common::{format_anyhow_error, now_ms, MAX_MANAGED_ICON_DATA_URL_BYTES};
use super::sync_commands::{
    auto_reconcile_skill_targets, move_managed_targets_to_trash, rollback_trashed_managed_paths,
    validated_managed_target,
};

#[derive(Debug, Serialize)]
pub struct UpdateResultDto {
    pub skill_id: String,
    pub name: String,
    pub content_hash: Option<String>,
    pub source_revision: Option<String>,
    pub updated_targets: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ManagedSkillDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_data_url: Option<String>,
    pub brand_color: Option<String>,
    pub source_type: String,
    pub source_ref: Option<String>,
    pub central_path: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_sync_at: Option<i64>,
    pub enabled: bool,
    pub has_external_source: bool,
    pub updateable: bool,
    pub status: String,
    pub tags: Vec<TagDto>,
    pub targets: Vec<SkillTargetDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagDto {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct TagWithCountDto {
    pub id: i64,
    pub name: String,
    pub skill_count: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
pub struct SkillTargetDto {
    pub tool: String,
    pub scope: String,
    pub project_path: Option<String>,
    pub mode: String,
    pub status: String,
    pub last_error: Option<String>,
    pub target_path: String,
    pub synced_at: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFileEntry {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SkillIgnoreItemDto {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_ignored: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SkillIgnoreConfigDto {
    pub rules: Vec<String>,
    pub items: Vec<SkillIgnoreItemDto>,
}

pub fn managed_skill_status(
    skill: &SkillRecord,
    update_capability: &ManagedSkillUpdateCapability,
) -> String {
    if skill.status != "ok" {
        return skill.status.clone();
    }
    if update_capability.integrity_error.is_some() {
        "error".to_string()
    } else {
        skill.status.clone()
    }
}

pub fn apply_managed_icon_data_url_budget(
    icon_data_url: &mut Option<String>,
    remaining_bytes: &mut usize,
) {
    let Some(encoded_icon) = icon_data_url.as_ref() else {
        return;
    };
    let encoded_bytes = encoded_icon.len();
    if encoded_bytes > *remaining_bytes {
        *icon_data_url = None;
        *remaining_bytes = 0;
        return;
    }
    *remaining_bytes -= encoded_bytes;
}

pub fn get_managed_skills_impl_with_icon_budget(
    store: &SkillStore,
    icon_data_url_budget: usize,
) -> Result<Vec<ManagedSkillDto>, String> {
    let skills = store.list_skills().map_err(|err| err.to_string())?;
    let mut remaining_icon_bytes = icon_data_url_budget;
    Ok(skills
        .into_iter()
        .map(|skill| {
            let mut ui_metadata = if remaining_icon_bytes == 0 {
                Default::default()
            } else {
                read_skill_ui_metadata(std::path::Path::new(&skill.central_path))
            };
            apply_managed_icon_data_url_budget(
                &mut ui_metadata.icon_data_url,
                &mut remaining_icon_bytes,
            );
            let update_capability = managed_skill_update_capability(&skill);
            let status = managed_skill_status(&skill, &update_capability);
            let targets = store
                .list_skill_targets(&skill.id)
                .unwrap_or_default()
                .into_iter()
                .map(|target| SkillTargetDto {
                    tool: target.tool,
                    scope: target.scope,
                    project_path: target.project_path,
                    mode: target.mode,
                    status: target.status,
                    last_error: target.last_error,
                    target_path: target.target_path,
                    synced_at: target.synced_at,
                })
                .collect();
            let tags = store
                .get_skill_tags(&skill.id)
                .unwrap_or_default()
                .into_iter()
                .map(|tag| TagDto {
                    id: tag.id,
                    name: tag.name,
                })
                .collect();

            ManagedSkillDto {
                id: skill.id,
                name: skill.name,
                description: skill.description,
                icon_data_url: ui_metadata.icon_data_url,
                brand_color: ui_metadata.brand_color,
                source_type: skill.source_type,
                source_ref: skill
                    .source_ref
                    .filter(|_| update_capability.source_ref_safe_to_expose),
                central_path: skill.central_path,
                created_at: skill.created_at,
                updated_at: skill.updated_at,
                last_sync_at: skill.last_sync_at,
                enabled: skill.enabled,
                has_external_source: update_capability.has_external_source,
                updateable: update_capability.updateable,
                status,
                tags,
                targets,
            }
        })
        .collect())
}

pub fn get_managed_skills_impl(store: &SkillStore) -> Result<Vec<ManagedSkillDto>, String> {
    get_managed_skills_impl_with_icon_budget(store, MAX_MANAGED_ICON_DATA_URL_BYTES)
}

#[tauri::command]
pub fn get_managed_skills(store: State<'_, SkillStore>) -> Result<Vec<ManagedSkillDto>, String> {
    get_managed_skills_impl(store.inner())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn set_skill_enabled(
    store: State<'_, SkillStore>,
    skillId: String,
    enabled: bool,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _mutation_guard = lock_central_mutation()?;
        if !enabled {
            let skill = store
                .get_skill_by_id(&skillId)?
                .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
            validate_skill_name(&skill.name)?;
            let targets = store.list_skill_targets(&skillId)?;
            let active_targets: Vec<_> = targets
                .iter()
                .filter(|target| target.status != "disabled")
                .collect();
            for target in &active_targets {
                validated_managed_target(&store, target, &skill.name)?;
            }
            let active_targets = active_targets.into_iter().cloned().collect::<Vec<_>>();
            let moved = move_managed_targets_to_trash(&store, &active_targets, &skill.name)?;
            let persist_result = (|| -> anyhow::Result<()> {
                for target in &targets {
                    store.update_skill_target_status(
                        &skillId,
                        &target.tool,
                        &target.scope,
                        target.project_path.as_deref(),
                        "disabled",
                    )?;
                }
                store.set_skill_enabled(&skillId, false)?;
                Ok(())
            })();
            if let Err(db_err) = persist_result {
                let mut recovery_errors = Vec::new();
                for original in &targets {
                    if let Err(err) = store.upsert_skill_target(original) {
                        recovery_errors.push(format!("restore DB target: {err:#}"));
                    }
                }
                if let Err(err) = store.set_skill_enabled(&skillId, skill.enabled) {
                    recovery_errors.push(format!("restore Skill state: {err:#}"));
                }
                if let Err(err) = rollback_trashed_managed_paths(&moved) {
                    recovery_errors.push(format!("restore files: {err:#}"));
                }
                if recovery_errors.is_empty() {
                    return Err(db_err)
                        .context("disable DB update failed; files and records restored");
                }
                anyhow::bail!(
                    "ROLLBACK_INCOMPLETE|disable DB update failed ({db_err:#}); {}",
                    recovery_errors.join(" | ")
                );
            }
            return Ok::<_, anyhow::Error>(());
        }

        anyhow::bail!(
            "USE_ATOMIC_ENABLE|Use enable_skill_and_restore_targets when enabling a Skill"
        )
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn update_managed_skill(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: String,
) -> Result<UpdateResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let res = update_managed_skill_from_source(&app, &store, &skillId)?;
        Ok::<_, anyhow::Error>(UpdateResultDto {
            skill_id: res.skill_id,
            name: res.name,
            content_hash: res.content_hash,
            source_revision: res.source_revision,
            updated_targets: res.updated_targets,
        })
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn delete_managed_skill(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: String,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _mutation_guard = lock_central_mutation()?;
        // 便于排查“按钮点了没反应”：确认前端确实触发了命令
        println!("[delete_managed_skill] skillId={}", skillId);

        let skill = store
            .get_skill_by_id(&skillId)?
            .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
        validate_skill_name(&skill.name)?;
        let central_root = resolve_central_repo_path(&app, &store)?;
        ensure_central_repo(&central_root)?;
        let central_path = std::path::PathBuf::from(&skill.central_path);
        validate_direct_skill_path(&central_root, &central_path)?;
        let expected_central = direct_skill_child(&central_root, &skill.name)?;
        if !paths_have_same_identity(&central_path, &expected_central)? {
            anyhow::bail!("UNSAFE_PATH|Stored central path does not match managed Skill name");
        }

        // Validate every target before moving anything. This prevents a corrupt
        // database row from turning a delete into an arbitrary filesystem move.
        let targets = store.list_skill_targets(&skillId)?;
        for target in &targets {
            validated_managed_target(&store, target, &skill.name)?;
        }

        let moved_targets = move_managed_targets_to_trash(&store, &targets, &skill.name)?;
        let trashed_central = match move_skill_to_trash(&central_root, &central_path) {
            Ok(Some(path)) => path,
            Ok(None) => {
                rollback_trashed_managed_paths(&moved_targets)?;
                anyhow::bail!("DELETE_FAILED|Central Skill disappeared before Trash move");
            }
            Err(err) => {
                rollback_trashed_managed_paths(&moved_targets).with_context(|| {
                    format!("central move failed ({err:#}) and target rollback was incomplete")
                })?;
                return Err(err).context("central move failed; targets were restored");
            }
        };
        if let Err(db_err) = store.delete_skill(&skillId) {
            let mut recovery_errors = Vec::new();
            if let Err(err) =
                restore_skill_from_trash(&central_root, &central_path, &trashed_central)
            {
                recovery_errors.push(format!("restore central Skill: {err:#}"));
            }
            if let Err(err) = rollback_trashed_managed_paths(&moved_targets) {
                recovery_errors.push(format!("restore targets: {err:#}"));
            }
            if recovery_errors.is_empty() {
                return Err(db_err).context("delete DB update failed; files were restored");
            }
            anyhow::bail!(
                "ROLLBACK_INCOMPLETE|delete DB update failed ({db_err:#}); {}",
                recovery_errors.join(" | ")
            );
        }

        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

pub fn validated_skill_content_root<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
    skill_id: &str,
) -> anyhow::Result<std::path::PathBuf> {
    let skill = store
        .get_skill_by_id(skill_id)?
        .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
    validate_skill_name(&skill.name)?;
    let central_root = resolve_central_repo_path(app, store)?;
    ensure_central_repo(&central_root)?;
    let stored = std::path::PathBuf::from(&skill.central_path);
    validate_direct_skill_path(&central_root, &stored)?;
    let expected = direct_skill_child(&central_root, &skill.name)?;
    if !paths_have_same_identity(&stored, &expected)? {
        anyhow::bail!("UNSAFE_PATH|Stored central path does not match managed Skill name");
    }
    Ok(stored)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn list_skill_files(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: Option<String>,
    skill_id: Option<String>,
    centralPath: Option<String>,
) -> Result<Vec<SkillFileEntry>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = if let Some(ref cp) = centralPath {
            let p = std::path::PathBuf::from(cp);
            if p.is_dir() {
                p
            } else {
                let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
                validated_skill_content_root(&app, &store, sid)?
            }
        } else {
            let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
            validated_skill_content_root(&app, &store, sid)?
        };
        let entries = crate::core::skill_files::list_files(&path)?;
        Ok::<_, anyhow::Error>(
            entries
                .into_iter()
                .map(|e| SkillFileEntry {
                    path: e.path,
                    size: e.size,
                })
                .collect(),
        )
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn read_skill_file(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: Option<String>,
    skill_id: Option<String>,
    centralPath: Option<String>,
    filePath: Option<String>,
    file_path: Option<String>,
) -> Result<String, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let base = if let Some(ref cp) = centralPath {
            let p = std::path::PathBuf::from(cp);
            if p.is_dir() {
                p
            } else {
                let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
                validated_skill_content_root(&app, &store, sid)?
            }
        } else {
            let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
            validated_skill_content_root(&app, &store, sid)?
        };
        let fp = filePath.as_deref().or(file_path.as_deref()).unwrap_or("");
        crate::core::skill_files::read_file(&base, fp)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn get_skill_ignore_config(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: Option<String>,
    skill_id: Option<String>,
    centralPath: Option<String>,
) -> Result<SkillIgnoreConfigDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = if let Some(ref cp) = centralPath {
            let p = std::path::PathBuf::from(cp);
            if p.is_dir() {
                p
            } else {
                let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
                validated_skill_content_root(&app, &store, sid)?
            }
        } else {
            let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
            validated_skill_content_root(&app, &store, sid)?
        };

        let skillignore_path = root.join(".skillignore");
        let mut rules = Vec::new();
        if let Ok(content) = std::fs::read_to_string(&skillignore_path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    rules.push(trimmed.to_string());
                }
            }
        }

        let mut items = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(&root) {
            for entry in read_dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name == ".git" || name == ".skillignore" {
                    continue;
                }
                let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                let clean_name = name.trim_matches('/').to_string();
                let is_ignored = rules.iter().any(|r| {
                    let clean_rule = r.replace('\\', "/").trim_matches('/').to_string();
                    clean_rule == clean_name
                        || (clean_rule.starts_with('*') && clean_name.ends_with(&clean_rule[1..]))
                        || (clean_rule.ends_with('*') && clean_name.starts_with(&clean_rule[..clean_rule.len().saturating_sub(1)]))
                });
                items.push(SkillIgnoreItemDto {
                    name,
                    path: if is_dir { format!("{}/", clean_name) } else { clean_name },
                    is_dir,
                    is_ignored,
                });
            }
        }
        items.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                b.is_dir.cmp(&a.is_dir)
            } else {
                a.name.cmp(&b.name)
            }
        });

        Ok::<_, anyhow::Error>(SkillIgnoreConfigDto { rules, items })
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn save_skill_ignore_config(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: Option<String>,
    skill_id: Option<String>,
    centralPath: Option<String>,
    rules: Vec<String>,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = if let Some(ref cp) = centralPath {
            let p = std::path::PathBuf::from(cp);
            if p.is_dir() {
                p
            } else {
                let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
                validated_skill_content_root(&app, &store, sid)?
            }
        } else {
            let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
            validated_skill_content_root(&app, &store, sid)?
        };

        let skillignore_path = root.join(".skillignore");
        
        let mut text = String::from("# Managed by Skills Hub\n");
        for rule in &rules {
            let trimmed = rule.trim();
            if !trimmed.is_empty() {
                text.push_str(trimmed);
                text.push('\n');
            }
        }
        std::fs::write(&skillignore_path, text)
            .with_context(|| format!("write .skillignore to {:?}", skillignore_path))?;

        // 重新计算哈希并更新 SQLite 记录
        if let Ok(new_hash) = crate::core::content_hash::hash_dir(&root) {
            let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
            if let Ok(Some(mut record)) = store.get_skill_by_id(sid) {
                record.content_hash = Some(new_hash);
                record.updated_at = now_ms();
                let _ = store.upsert_skill(&record);
            }
            let _ = auto_reconcile_skill_targets(&store, Some(sid));
        }

        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub fn get_tags(store: State<'_, SkillStore>) -> Result<Vec<TagWithCountDto>, String> {
    store
        .list_tags_with_counts()
        .map(|tags| {
            tags.into_iter()
                .map(|tag| TagWithCountDto {
                    id: tag.id,
                    name: tag.name,
                    skill_count: tag.skill_count,
                    updated_at: tag.updated_at,
                })
                .collect()
        })
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn create_tag(store: State<'_, SkillStore>, name: String) -> Result<TagDto, String> {
    store
        .create_tag(&name)
        .map(|tag| TagDto {
            id: tag.id,
            name: tag.name,
        })
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn rename_tag(
    store: State<'_, SkillStore>,
    tagId: i64,
    name: String,
) -> Result<TagDto, String> {
    store
        .rename_tag(tagId, &name)
        .map(|tag| TagDto {
            id: tag.id,
            name: tag.name,
        })
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn delete_tag(store: State<'_, SkillStore>, tagId: i64) -> Result<(), String> {
    store.delete_tag(tagId).map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn get_skill_tags(
    store: State<'_, SkillStore>,
    skillId: String,
) -> Result<Vec<TagDto>, String> {
    store
        .get_skill_tags(&skillId)
        .map(|tags| {
            tags.into_iter()
                .map(|tag| TagDto {
                    id: tag.id,
                    name: tag.name,
                })
                .collect()
        })
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn set_skill_tags(
    store: State<'_, SkillStore>,
    skillId: String,
    tagIds: Vec<i64>,
) -> Result<(), String> {
    store
        .set_skill_tags(&skillId, &tagIds)
        .map_err(format_anyhow_error)
}

#[tauri::command]
pub fn get_untagged_skill_ids(store: State<'_, SkillStore>) -> Result<Vec<String>, String> {
    store.list_untagged_skill_ids().map_err(format_anyhow_error)
}
