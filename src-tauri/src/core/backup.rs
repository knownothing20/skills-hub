use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use crate::core::central_repo::resolve_central_repo_path;
use crate::core::skill_store::SkillStore;

const BACKUP_CONFIG_KEY: &str = "backup_config_v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    pub enabled: bool,
    pub backup_dir: String,
    pub last_backup_time: Option<String>,
    pub last_backup_count: Option<usize>,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            backup_dir: "D:\\GitHub\\skill-hub\\backup".to_string(),
            last_backup_time: None,
            last_backup_count: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupTargetItem {
    pub tool: String,
    pub target_path: String,
    pub status: String,
    pub scope: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupManifestItem {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub tags: Vec<String>,
    pub source_ref: Option<String>,
    pub targets: Vec<BackupTargetItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupManifest {
    pub version: String,
    pub backup_time: String,
    pub skill_count: usize,
    pub central_path: String,
    pub skills: Vec<BackupManifestItem>,
}

pub fn get_backup_config(store: &SkillStore) -> BackupConfig {
    store
        .get_setting(BACKUP_CONFIG_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_backup_config(store: &SkillStore, config: &BackupConfig) -> Result<()> {
    let raw = serde_json::to_string(config)?;
    store.set_setting(BACKUP_CONFIG_KEY, &raw)?;
    Ok(())
}

fn copy_dir_all(src: &Path, dst: &Path) {
    if !src.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(dst);
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name();
            let file_name_lower = file_name.to_string_lossy().to_lowercase();
            if file_name_lower == ".git"
                || file_name_lower == "node_modules"
                || file_name_lower == "venv"
                || file_name_lower == ".venv"
                || file_name_lower == "__pycache__"
                || file_name_lower == ".skills-hub-staging"
                || file_name_lower == ".cache"
                || file_name_lower.ends_with(".pyc")
            {
                continue;
            }
            let dest_child = dst.join(&file_name);
            if path.is_dir() {
                copy_dir_all(&path, &dest_child);
            } else if path.is_file() {
                let should_copy = if dest_child.exists() {
                    let src_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                    let dst_mtime = std::fs::metadata(&dest_child).and_then(|m| m.modified()).ok();
                    match (src_mtime, dst_mtime) {
                        (Some(sm), Some(dm)) => sm > dm,
                        _ => true,
                    }
                } else {
                    true
                };
                if should_copy {
                    let _ = std::fs::copy(&path, &dest_child);
                }
            }
        }
    }
}

pub fn perform_backup<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
    custom_dir: Option<String>,
) -> Result<BackupManifest> {
    let mut config = get_backup_config(store);
    let target_dir_str = custom_dir.unwrap_or(config.backup_dir.clone());
    let backup_path = PathBuf::from(&target_dir_str);
    std::fs::create_dir_all(&backup_path)
        .with_context(|| format!("failed to create backup dir {:?}", backup_path))?;

    let central_root = resolve_central_repo_path(app, store)?;
    let dest_skills = backup_path.join("skills");
    copy_dir_all(&central_root, &dest_skills);

    let dest_db_dir = backup_path.join("database");
    let _ = std::fs::create_dir_all(&dest_db_dir);
    if let Ok(db_path) = crate::core::skill_store::default_db_path(app) {
        if db_path.exists() {
            let dest_db = dest_db_dir.join("skills_hub.db");
            let _ = std::fs::copy(&db_path, &dest_db);
            let wal_path = db_path.with_file_name("skills_hub.db-wal");
            if wal_path.exists() {
                let _ = std::fs::copy(&wal_path, dest_db_dir.join("skills_hub.db-wal"));
            }
        }
    }

    let all_skills = store.list_skills()?;
    let mut manifest_skills = Vec::new();

    for skill in &all_skills {
        let targets = store
            .list_skill_targets(&skill.id)?
            .into_iter()
            .map(|t| BackupTargetItem {
                tool: t.tool,
                target_path: t.target_path,
                status: t.status,
                scope: t.scope,
            })
            .collect();

        let tags = store
            .get_skill_tags(&skill.id)?
            .into_iter()
            .map(|t| t.name)
            .collect();

        manifest_skills.push(BackupManifestItem {
            id: skill.id.clone(),
            name: skill.name.clone(),
            description: skill.description.clone(),
            enabled: skill.enabled,
            tags,
            source_ref: skill.source_ref.clone(),
            targets,
        });
    }

    let now_str = store.get_local_now_string().unwrap_or_else(|_| "2026-09-03 00:00:00".to_string());
    let manifest = BackupManifest {
        version: "1.0".to_string(),
        backup_time: now_str,
        skill_count: manifest_skills.len(),
        central_path: central_root.to_string_lossy().to_string(),
        skills: manifest_skills,
    };

    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(backup_path.join("manifest.json"), manifest_json)
        .with_context(|| "failed to write manifest.json")?;

    config.last_backup_time = Some(manifest.backup_time.clone());
    config.last_backup_count = Some(manifest.skill_count);
    config.backup_dir = target_dir_str;
    let _ = save_backup_config(store, &config);

    Ok(manifest)
}

pub fn perform_restore<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
    source_dir: Option<String>,
) -> Result<usize> {
    let config = get_backup_config(store);
    let backup_dir_str = source_dir.unwrap_or(config.backup_dir);
    let backup_path = PathBuf::from(&backup_dir_str);

    let src_skills = backup_path.join("skills");
    if !src_skills.exists() {
        anyhow::bail!("备份目录中未发现 skills 文件夹: {:?}", src_skills);
    }

    let central_root = resolve_central_repo_path(app, store)?;
    copy_dir_all(&src_skills, &central_root);

    let src_db = backup_path.join("database").join("skills_hub.db");
    if src_db.exists() {
        if let Ok(db_path) = crate::core::skill_store::default_db_path(app) {
            let bak_old = db_path.with_extension(format!("bak-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()));
            let _ = std::fs::copy(&db_path, &bak_old);
            let _ = std::fs::copy(&src_db, &db_path);
        }
    }

    let mut restored_count = 0;
    if let Ok(entries) = std::fs::read_dir(&central_root) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                restored_count += 1;
            }
        }
    }

    Ok(restored_count)
}
