use std::path::Path;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use walkdir::{DirEntry, WalkDir};

const DEFAULT_IGNORES: &[&str] = &[
    ".git",
    ".DS_Store",
    "Thumbs.db",
    ".gitignore",
    ".skillignore",
    "node_modules",
    "__pycache__",
    ".pytest_cache",
    ".cache",
    ".tmp",
    ".agentport-tmp",
    ".codex-tmp",
    "tmp",
];

pub(crate) fn load_ignore_patterns(root: &Path) -> Vec<String> {
    let mut patterns: Vec<String> = DEFAULT_IGNORES
        .iter()
        .map(|&s| s.trim_matches('/').to_string())
        .collect();

    let skillignore_path = root.join(".skillignore");
    if let Ok(content) = std::fs::read_to_string(&skillignore_path) {
        for line in content.lines() {
            let trimmed = line.trim().trim_start_matches('\u{feff}').trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let normalized = trimmed.replace('\\', "/");
            let clean = normalized.trim_matches('/').to_string();
            if !clean.is_empty() && !patterns.iter().any(|p| p == &clean) {
                patterns.push(clean);
            }
        }
    }

    patterns
}

fn matches_rule(pattern: &str, relative_norm: &str, file_name: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }

    // 1. 完全匹配文件名或相对路径
    if file_name == pattern || relative_norm == pattern {
        return true;
    }

    // 2. 匹配目录层级（例如 pattern 为 "local"，则 "local/..." 或以 "/local/" 结尾或包含 "/local/"）
    if relative_norm.starts_with(&format!("{}/", pattern))
        || relative_norm.contains(&format!("/{}/", pattern))
        || relative_norm.ends_with(&format!("/{}", pattern))
    {
        return true;
    }

    // 3. 通配符后缀匹配（如 *.key, *.log）
    if pattern.starts_with('*') {
        let suffix = &pattern[1..];
        if file_name.ends_with(suffix) || relative_norm.ends_with(suffix) {
            return true;
        }
    }

    // 4. 通配符前缀匹配（如 connections*.json）
    if pattern.ends_with('*') {
        let prefix = &pattern[..pattern.len() - 1];
        if file_name.starts_with(prefix) {
            return true;
        }
    }

    false
}

pub(crate) fn is_entry_ignored(root: &Path, entry: &DirEntry, rules: &[String]) -> bool {
    let relative = match entry.path().strip_prefix(root) {
        Ok(rel) => rel,
        Err(_) => return false,
    };

    let relative_str = relative.to_string_lossy();
    if relative_str.is_empty() {
        return false;
    }

    let relative_norm = relative_str.replace('\\', "/");
    let file_name = entry.file_name().to_string_lossy();

    for rule in rules {
        if matches_rule(rule, &relative_norm, &file_name) {
            return true;
        }
    }

    false
}

pub fn hash_dir(path: &Path) -> Result<String> {
    let rules = load_ignore_patterns(path);
    let mut hasher = Sha256::new();

    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !is_entry_ignored(path, entry, &rules))
    {
        let entry = entry?;
        if is_entry_ignored(path, &entry, &rules) {
            continue;
        }

        let relative = entry
            .path()
            .strip_prefix(path)
            .with_context(|| format!("strip prefix {:?}", entry.path()))?;
        
        let relative_norm = relative.to_string_lossy().replace('\\', "/");
        hasher.update(relative_norm.as_bytes());

        if entry.file_type().is_file() {
            let bytes = std::fs::read(entry.path())
                .with_context(|| format!("read file {:?}", entry.path()))?;
            hasher.update(bytes);
        }
    }

    let digest = hasher.finalize();
    Ok(hex::encode(digest))
}

#[cfg(test)]
#[path = "tests/content_hash.rs"]
mod tests;
