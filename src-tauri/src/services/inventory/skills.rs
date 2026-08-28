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

pub(crate) trait DisabledSkillPaths {
    fn contains_skill(&self, original: &Path, resolved: &Path) -> bool;
}

impl DisabledSkillPaths for HashSet<String> {
    fn contains_skill(&self, original: &Path, resolved: &Path) -> bool {
        self.contains(&original.display().to_string())
            || self.contains(&resolved.display().to_string())
    }
}

impl DisabledSkillPaths for [PathBuf] {
    fn contains_skill(&self, original: &Path, _resolved: &Path) -> bool {
        self.iter()
            .any(|disabled_path| codex_paths_match(disabled_path, original))
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn scan_skill_root<D: DisabledSkillPaths + ?Sized>(
    root: &Path,
    client: ClientKind,
    scope: InventoryScope,
    source_kind: SourceKind,
    project_path: Option<&Path>,
    source_priority: u16,
    require_frontmatter_name: bool,
    source_enabled: bool,
    trust_state: TrustState,
    disabled_paths: &D,
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
        let original_path_is_lossless =
            Path::new(&original_path).as_os_str() == skill_file.as_os_str();
        let resolved_path = resolved.display().to_string();
        let enabled = source_enabled && !disabled_paths.contains_skill(skill_file, &resolved);
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
        if !has_required_name || !original_path_is_lossless {
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

pub fn codex_disabled_skill_paths(config: Option<&toml::Value>) -> Vec<PathBuf> {
    let mut disabled = Vec::new();
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
            disabled.push(candidate);
        }
    }
    disabled.sort();
    disabled.dedup();
    disabled
}

pub(super) fn codex_paths_match(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        paths_identify_the_same_directory_entries(left, right)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[cfg(target_os = "macos")]
fn paths_identify_the_same_directory_entries(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    if left.is_absolute() != right.is_absolute() {
        return false;
    }
    let mut left_components = left.components();
    let mut right_components = right.components();
    let mut left_prefix = PathBuf::new();
    let mut right_prefix = PathBuf::new();
    loop {
        match (left_components.next(), right_components.next()) {
            (None, None) => return true,
            (Some(left_component), Some(right_component)) => {
                left_prefix.push(left_component.as_os_str());
                right_prefix.push(right_component.as_os_str());
                if left_component == right_component {
                    continue;
                }
                let (Ok(left_metadata), Ok(right_metadata)) = (
                    std::fs::symlink_metadata(&left_prefix),
                    std::fs::symlink_metadata(&right_prefix),
                ) else {
                    return false;
                };
                if left_metadata.dev() != right_metadata.dev()
                    || left_metadata.ino() != right_metadata.ino()
                {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

pub fn codex_skill_config_is_editable(config: Option<&toml::Value>) -> bool {
    let Some(config) = config else {
        return true;
    };
    let Some(skills) = config.get("skills") else {
        return true;
    };
    let Some(skills) = skills.as_table() else {
        return false;
    };
    let Some(entries) = skills.get("config") else {
        return true;
    };
    let Some(entries) = entries.as_array() else {
        return false;
    };
    entries.iter().all(|entry| {
        let Some(entry) = entry.as_table() else {
            return false;
        };
        let valid_path = entry
            .get("path")
            .and_then(toml::Value::as_str)
            .is_some_and(|path| !path.trim().is_empty());
        let valid_enabled = entry
            .get("enabled")
            .is_none_or(|enabled| enabled.as_bool().is_some());
        valid_path && valid_enabled
    })
}

fn skill_config_path_candidates(path: &Path) -> Vec<PathBuf> {
    if path.is_dir() {
        vec![path.to_path_buf(), path.join("SKILL.md")]
    } else if path.file_name().is_some_and(|name| name == "SKILL.md") {
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
    use super::{codex_skill_config_is_editable, parse_frontmatter_name};

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
    fn rejects_malformed_codex_skill_entries() {
        let missing_path: toml::Value =
            toml::from_str("[[skills.config]]\nenabled = false\n").unwrap();
        let invalid_enabled: toml::Value =
            toml::from_str("[[skills.config]]\npath = '/tmp/skill'\nenabled = 'no'\n").unwrap();

        assert!(!codex_skill_config_is_editable(Some(&missing_path)));
        assert!(!codex_skill_config_is_editable(Some(&invalid_enabled)));
    }

    #[cfg(unix)]
    #[test]
    fn a_lossy_skill_path_does_not_round_trip() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let path = std::path::PathBuf::from(OsString::from_vec(b"skill-\xff/SKILL.md".to_vec()));
        let rendered = path.display().to_string();

        assert_ne!(
            std::path::Path::new(&rendered).as_os_str(),
            path.as_os_str()
        );
    }

    #[test]
    fn rejects_unclosed_frontmatter() {
        assert_eq!(parse_frontmatter_name("---\nname: unsafe\nBody"), None);
    }
}
