use super::config::paths_differ;
use super::models::{
    effective_state, ActionBlockedReason, ClientKind, InventoryItemType, InventoryRecord,
    InventoryScope, InventorySnapshot, InventoryWarning, SourceKind, TrustState,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

const MAX_SKILL_FILE_BYTES: u64 = 1024 * 1024;
const MAX_SKILL_SCAN_DEPTH: usize = 2;

#[allow(clippy::too_many_arguments)]
pub fn scan_skill_root(
    root: &Path,
    client: ClientKind,
    scope: InventoryScope,
    source_kind: SourceKind,
    project_path: Option<&Path>,
    source_priority: u16,
    require_frontmatter_name: bool,
    source_enabled: bool,
    trust_state: TrustState,
    disabled_paths: &HashSet<String>,
    snapshot: &mut InventorySnapshot,
) {
    match std::fs::symlink_metadata(root) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            snapshot.warnings.push(InventoryWarning::new(
                client,
                root.display().to_string(),
                "Could not inspect this skill directory.",
            ));
            return;
        }
    }

    let walker = WalkDir::new(root)
        .follow_links(true)
        .max_depth(MAX_SKILL_SCAN_DEPTH)
        .into_iter()
        .filter_entry(should_descend);
    let mut ordinal = 0;
    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                let error_path = error.path().unwrap_or(root);
                let is_broken_symlink = std::fs::symlink_metadata(error_path)
                    .map(|metadata| metadata.file_type().is_symlink())
                    .unwrap_or(false)
                    && std::fs::metadata(error_path).is_err();
                snapshot.warnings.push(if is_broken_symlink {
                    InventoryWarning::blocked(
                        client,
                        error_path.display().to_string(),
                        "Skipped a broken skill symlink.",
                        ActionBlockedReason::BrokenSymlink,
                    )
                } else {
                    InventoryWarning::new(
                        client,
                        error_path.display().to_string(),
                        "Skipped an unreadable or cyclic skill path.",
                    )
                });
                continue;
            }
        };
        if !entry.file_type().is_file() || entry.file_name() != "SKILL.md" {
            continue;
        }
        let skill_file = entry.path();
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => {
                push_skill_warning(
                    client,
                    skill_file,
                    "Could not read this skill file.",
                    snapshot,
                );
                continue;
            }
        };
        if metadata.len() > MAX_SKILL_FILE_BYTES {
            push_skill_warning(
                client,
                skill_file,
                "Skipped this skill because SKILL.md is larger than 1 MB.",
                snapshot,
            );
            continue;
        }
        let content = match std::fs::read_to_string(skill_file) {
            Ok(content) => content,
            Err(_) => {
                push_skill_warning(
                    client,
                    skill_file,
                    "Could not read this skill file.",
                    snapshot,
                );
                continue;
            }
        };
        snapshot.record_source_revision(skill_file, content.as_bytes());
        let folder_name = skill_file
            .parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unnamed skill".to_string());
        let frontmatter_name = parse_frontmatter_name(&content);
        let has_required_name = !require_frontmatter_name || frontmatter_name.is_some();
        if !has_required_name {
            push_skill_warning(
                client,
                skill_file,
                "This skill is missing a frontmatter name; its folder name is shown instead.",
                snapshot,
            );
        }
        let name = frontmatter_name.unwrap_or(folder_name);
        let original_path = skill_file.display().to_string();
        let resolved = match std::fs::canonicalize(skill_file) {
            Ok(path) => path,
            Err(_) => {
                push_skill_warning(
                    client,
                    skill_file,
                    "Could not resolve this skill path.",
                    snapshot,
                );
                continue;
            }
        };
        let resolved_path = resolved.display().to_string();
        let enabled = source_enabled
            && !disabled_paths.contains(&original_path)
            && !disabled_paths.contains(&resolved_path);
        let mut record = InventoryRecord::new(
            client,
            InventoryItemType::Skill,
            name,
            scope,
            source_kind,
            original_path.clone(),
            project_path,
            ordinal,
            source_priority,
        );
        record.original_path = original_path.clone();
        record.resolved_path = Some(resolved_path.clone());
        record.is_symlink = paths_differ(skill_file, &resolved);
        record.enabled = Some(enabled);
        record.trust_state = trust_state;
        record.is_effective = effective_state(enabled && has_required_name, trust_state);
        if !has_required_name {
            record.restrict_actions(ActionBlockedReason::MalformedSource);
        }
        snapshot.records.push(record);
        ordinal += 1;
    }
}

pub fn discover_project_skill_roots(project_root: &Path, parent_names: &[&str]) -> Vec<PathBuf> {
    if !project_root.is_dir() {
        return Vec::new();
    }
    let mut roots: Vec<PathBuf> = parent_names
        .iter()
        .map(|parent_name| project_root.join(parent_name).join("skills"))
        .filter(|path| path.is_dir())
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

pub fn codex_disabled_skill_paths(config: Option<&toml::Value>) -> HashSet<String> {
    let mut disabled = HashSet::new();
    let Some(entries) = config
        .and_then(|value| value.get("skills"))
        .and_then(|value| value.get("config"))
        .and_then(toml::Value::as_array)
    else {
        return disabled;
    };
    for entry in entries {
        let Some(table) = entry.as_table() else {
            continue;
        };
        if table.get("enabled").and_then(toml::Value::as_bool) != Some(false) {
            continue;
        }
        let Some(path) = table.get("path").and_then(toml::Value::as_str) else {
            continue;
        };
        let path = PathBuf::from(path);
        for candidate in skill_config_path_candidates(&path) {
            disabled.insert(candidate.display().to_string());
        }
    }
    disabled
}

fn skill_config_path_candidates(path: &Path) -> Vec<PathBuf> {
    if path.file_name().is_some_and(|name| name == "SKILL.md") {
        vec![path.to_path_buf()]
    } else {
        vec![path.to_path_buf(), path.join("SKILL.md")]
    }
}

fn parse_frontmatter_name(content: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut name = None;
    for line in lines {
        let line = line.trim();
        if line == "---" {
            return name;
        }
        let Some(value) = line.strip_prefix("name:") else {
            continue;
        };
        let value = value.trim().trim_matches(['\'', '"']);
        if !value.is_empty() {
            name = Some(value.to_string());
        }
    }
    None
}

fn should_descend(entry: &DirEntry) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_dir() {
        return true;
    }
    let directory_name = entry.file_name().to_string_lossy();
    !matches!(directory_name.as_ref(), ".git" | "node_modules" | ".venv")
}

fn push_skill_warning(
    client: ClientKind,
    path: &Path,
    message: &str,
    snapshot: &mut InventorySnapshot,
) {
    snapshot.warnings.push(InventoryWarning::new(
        client,
        path.display().to_string(),
        message,
    ));
}

#[cfg(test)]
mod tests {
    use super::parse_frontmatter_name;

    #[test]
    fn parses_quoted_frontmatter_name() {
        let content = "---\nname: \"safe-skill\"\ndescription: Test\n---\nBody";
        assert_eq!(
            parse_frontmatter_name(content).as_deref(),
            Some("safe-skill")
        );
    }

    #[test]
    fn ignores_name_outside_frontmatter() {
        assert_eq!(parse_frontmatter_name("# Skill\nname: unsafe"), None);
    }

    #[test]
    fn rejects_unclosed_frontmatter() {
        assert_eq!(parse_frontmatter_name("---\nname: unsafe\nBody"), None);
    }
}
