use std::path::{Path, PathBuf};

use super::skill_store::SkillStore;
use anyhow::{Context, Result};
use dirs::home_dir;

const AGENTS_SKILLS_DIR: &str = ".agents/skills";

pub fn fixed_central_repo_path() -> Result<PathBuf> {
    let home = home_dir().context("failed to resolve home directory for ~/.agents/skills")?;
    Ok(home.join(AGENTS_SKILLS_DIR))
}

pub fn resolve_central_repo_path<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    store: &SkillStore,
) -> Result<PathBuf> {
    // Unit tests use a temporary override so they never touch the user's live
    // Skills. Safe production builds intentionally ignore the upstream mutable
    // setting: ~/.agents/skills is the one and only source of truth.
    #[cfg(test)]
    if let Some(path) = store.get_setting("central_repo_path")? {
        return Ok(PathBuf::from(path));
    }

    #[cfg(not(test))]
    let _ = store;

    fixed_central_repo_path()
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
