use std::path::{Path, PathBuf};

use super::skill_store::SkillStore;
use anyhow::{Context, Result};
use dirs::home_dir;

pub fn app_install_skills_dir() -> PathBuf {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            // 如果处于开发/源码打包环境 (dist-app 或 target/...)
            if let Some(parent) = exe_dir.parent() {
                if parent.join("package.json").exists() || parent.join(".git").exists() {
                    return parent.join("skills");
                }
                if let Some(grandparent) = parent.parent() {
                    if grandparent.join("package.json").exists() || grandparent.join(".git").exists() {
                        return grandparent.join("skills");
                    }
                }
            }
            return exe_dir.join("skills");
        }
    }
    PathBuf::from("D:\\GitHub\\skill-hub\\skills")
}

pub fn fixed_central_repo_path() -> Result<PathBuf> {
    Ok(app_install_skills_dir())
}

pub fn resolve_central_repo_path<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    store: &SkillStore,
) -> Result<PathBuf> {
    if let Some(path) = store.get_setting("central_repo_path")? {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }

    fixed_central_repo_path()
}

pub fn migrate_legacy_central_if_needed<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
) -> Result<()> {
    let new_central = resolve_central_repo_path(app, store)?;
    ensure_central_repo(&new_central)?;

    let legacy_central = match home_dir() {
        Some(h) => h.join(".agents").join("skills"),
        None => return Ok(()),
    };

    if legacy_central.exists() && legacy_central != new_central {
        log::info!(
            "migrating legacy central skills from {:?} to {:?}",
            legacy_central,
            new_central
        );

        // 将旧公共目录中的技能复制到新私有安装母版中
        if let Ok(entries) = std::fs::read_dir(&legacy_central) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = entry.file_name();
                    let target = new_central.join(&name);
                    if !target.exists() {
                        let _ = crate::core::backup::copy_dir_all(&path, &target);
                    }
                }
            }
        }

        // 更新数据库中所有 skills 的 central_path 为新安装目录路径
        let legacy_norm = legacy_central.to_string_lossy().replace('/', "\\");
        if let Ok(skills) = store.list_skills() {
            for mut skill in skills {
                let skill_norm = skill.central_path.replace('/', "\\");
                if skill_norm.starts_with(&legacy_norm) {
                    let sub = &skill_norm[legacy_norm.len()..];
                    let sub = sub.trim_start_matches('\\');
                    skill.central_path = new_central.join(sub).to_string_lossy().to_string();
                    let _ = store.upsert_skill(&skill);
                }
            }
        }
    }

    Ok(())
}

pub fn ensure_central_repo(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("central Skill root has no parent"))?;
    match std::fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            anyhow::bail!("UNSAFE_PATH|Central Skill parent must be a real directory");
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create central Skill parent {:?}", parent))?;
            let metadata = std::fs::symlink_metadata(parent)
                .with_context(|| format!("verify central Skill parent {:?}", parent))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                anyhow::bail!("UNSAFE_PATH|Central Skill parent must be a real directory");
            }
        }
        Err(err) => return Err(err).with_context(|| format!("stat central parent {:?}", parent)),
    }

    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            anyhow::bail!("UNSAFE_PATH|Central Skill root must be a real directory");
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(path).with_context(|| format!("create {:?}", path))?;
        }
        Err(err) => return Err(err).with_context(|| format!("stat central root {:?}", path)),
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/central_repo.rs"]
mod tests;
