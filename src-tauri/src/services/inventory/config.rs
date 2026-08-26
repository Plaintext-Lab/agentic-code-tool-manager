use super::models::{
    effective_state, ActionBlockedReason, ClientKind, InventoryItemType, InventoryRecord,
    InventoryScope, InventorySnapshot, InventoryWarning, SourceKind, TrustState,
};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::path::Path;

const MAX_CONFIG_BYTES: u64 = 5 * 1024 * 1024;

pub fn read_json(
    path: &Path,
    client: ClientKind,
    snapshot: &mut InventorySnapshot,
) -> Option<Value> {
    let content = read_config(path, client, "JSON", snapshot)?;
    match serde_json::from_str(&content) {
        Ok(value) => Some(value),
        Err(_) => {
            snapshot.restrict_source(
                &path.display().to_string(),
                ActionBlockedReason::MalformedSource,
            );
            snapshot.warnings.push(InventoryWarning::new(
                client,
                path.display().to_string(),
                "Could not parse this JSON configuration.",
            ));
            None
        }
    }
}

pub fn read_toml(
    path: &Path,
    client: ClientKind,
    snapshot: &mut InventorySnapshot,
) -> Option<toml::Value> {
    let content = read_config(path, client, "TOML", snapshot)?;
    match toml::from_str(&content) {
        Ok(value) => Some(value),
        Err(_) => {
            snapshot.restrict_source(
                &path.display().to_string(),
                ActionBlockedReason::MalformedSource,
            );
            snapshot.warnings.push(InventoryWarning::new(
                client,
                path.display().to_string(),
                "Could not parse this TOML configuration.",
            ));
            None
        }
    }
}

fn read_config(
    path: &Path,
    client: ClientKind,
    format_name: &str,
    snapshot: &mut InventorySnapshot,
) -> Option<String> {
    let link_metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            snapshot.record_source_absence(path);
            return None;
        }
        Err(_) => {
            snapshot.warnings.push(InventoryWarning::new(
                client,
                path.display().to_string(),
                format!("Could not read this {format_name} configuration."),
            ));
            return None;
        }
    };
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && link_metadata.file_type().is_symlink() =>
        {
            snapshot.record_source_absence(path);
            snapshot.restrict_source(
                &path.display().to_string(),
                ActionBlockedReason::BrokenSymlink,
            );
            snapshot.warnings.push(InventoryWarning::blocked(
                client,
                path.display().to_string(),
                format!(
                    "Skipped this {format_name} configuration because its symlink target is unavailable."
                ),
                ActionBlockedReason::BrokenSymlink,
            ));
            return None;
        }
        Err(_) => {
            snapshot.restrict_source(
                &path.display().to_string(),
                ActionBlockedReason::StateUnavailable,
            );
            snapshot.warnings.push(InventoryWarning::new(
                client,
                path.display().to_string(),
                format!("Could not read this {format_name} configuration."),
            ));
            return None;
        }
    };
    if metadata.len() > MAX_CONFIG_BYTES {
        snapshot.restrict_source(
            &path.display().to_string(),
            ActionBlockedReason::MalformedSource,
        );
        snapshot.warnings.push(InventoryWarning::new(
            client,
            path.display().to_string(),
            format!("Skipped this {format_name} configuration because it is larger than 5 MB."),
        ));
        return None;
    }
    match std::fs::read_to_string(path) {
        Ok(content) => {
            snapshot.record_source_revision(path, content.as_bytes());
            Some(content)
        }
        Err(_) => {
            snapshot.restrict_source(
                &path.display().to_string(),
                ActionBlockedReason::StateUnavailable,
            );
            snapshot.warnings.push(InventoryWarning::new(
                client,
                path.display().to_string(),
                format!("Could not read this {format_name} configuration."),
            ));
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn push_json_mcps(
    servers: &Map<String, Value>,
    config_path: &Path,
    client: ClientKind,
    scope: InventoryScope,
    source_kind: SourceKind,
    project_path: Option<&Path>,
    source_priority: u16,
    disabled_names: &HashSet<String>,
    approved_names: Option<&HashSet<String>>,
    policy_blocked_names: &HashSet<String>,
    trust_state: TrustState,
    snapshot: &mut InventorySnapshot,
) {
    for (ordinal, (name, value)) in servers.iter().enumerate() {
        let Some(config) = value.as_object() else {
            snapshot.warnings.push(InventoryWarning::new(
                client,
                config_path.display().to_string(),
                "Skipped an MCP entry because it is not an object.",
            ));
            continue;
        };
        let enabled = config
            .get("enabled")
            .and_then(Value::as_bool)
            .or_else(|| {
                config
                    .get("disabled")
                    .and_then(Value::as_bool)
                    .map(|value| !value)
            })
            .unwrap_or_else(|| !disabled_names.contains(name));
        let Some(detail) = json_transport_detail(config) else {
            snapshot.warnings.push(InventoryWarning::new(
                client,
                config_path.display().to_string(),
                "Skipped an MCP entry because it has no usable transport.",
            ));
            continue;
        };
        let mut record = InventoryRecord::new(
            client,
            InventoryItemType::Mcp,
            name.clone(),
            scope,
            source_kind,
            config_path.display().to_string(),
            project_path,
            ordinal,
            source_priority,
        );
        apply_path_metadata(&mut record, config_path);
        record.enabled = Some(enabled);
        record.trust_state = trust_state;
        let approved = approved_names.is_none_or(|names| names.contains(name));
        record.approval_pending = enabled && approved_names.is_some() && !approved;
        record.is_effective = if enabled && (!approved || policy_blocked_names.contains(name)) {
            Some(false)
        } else {
            effective_state(enabled, trust_state)
        };
        record.protected_fields = json_protected_fields(config);
        record.detail = Some(detail.to_string());
        if policy_blocked_names.contains(name) {
            record.restrict_actions(ActionBlockedReason::PolicyControlled);
        }
        snapshot.records.push(record);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn push_toml_mcps(
    config: &toml::Value,
    config_path: &Path,
    client: ClientKind,
    scope: InventoryScope,
    source_kind: SourceKind,
    project_path: Option<&Path>,
    source_priority: u16,
    trust_state: TrustState,
    snapshot: &mut InventorySnapshot,
) {
    let Some(servers) = config.get("mcp_servers").and_then(toml::Value::as_table) else {
        return;
    };
    for (ordinal, (name, value)) in servers.iter().enumerate() {
        let Some(server) = value.as_table() else {
            snapshot.warnings.push(InventoryWarning::new(
                client,
                config_path.display().to_string(),
                "Skipped an MCP entry because it is not a table.",
            ));
            continue;
        };
        let enabled = server
            .get("enabled")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);
        let Some(detail) = toml_transport_detail(server) else {
            snapshot.warnings.push(InventoryWarning::new(
                client,
                config_path.display().to_string(),
                "Skipped an MCP entry because it has no usable transport.",
            ));
            continue;
        };
        let mut record = InventoryRecord::new(
            client,
            InventoryItemType::Mcp,
            name.clone(),
            scope,
            source_kind,
            config_path.display().to_string(),
            project_path,
            ordinal,
            source_priority,
        );
        apply_path_metadata(&mut record, config_path);
        record.enabled = Some(enabled);
        record.trust_state = trust_state;
        record.is_effective = effective_state(enabled, trust_state);
        record.protected_fields = toml_protected_fields(server);
        record.detail = Some(detail.to_string());
        snapshot.records.push(record);
    }
}

pub fn apply_path_metadata(record: &mut InventoryRecord, path: &Path) {
    if let Ok(resolved) = std::fs::canonicalize(path) {
        record.is_symlink = paths_differ(Path::new(&record.original_path), &resolved);
        record.resolved_path = Some(resolved.display().to_string());
    }
}

fn json_transport_detail(config: &Map<String, Value>) -> Option<&'static str> {
    if config
        .get("url")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Some("HTTP MCP server");
    }
    config
        .get("command")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|_| "STDIO MCP server")
}

fn toml_transport_detail(config: &toml::map::Map<String, toml::Value>) -> Option<&'static str> {
    if config
        .get("url")
        .and_then(toml::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Some("HTTP MCP server");
    }
    config
        .get("command")
        .and_then(toml::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|_| "STDIO MCP server")
}

pub(super) fn paths_differ(original: &Path, resolved: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        normalize_windows_path(&original.to_string_lossy())
            != normalize_windows_path(&resolved.to_string_lossy())
    }
    #[cfg(not(target_os = "windows"))]
    {
        original != resolved
    }
}

#[cfg(any(test, target_os = "windows"))]
fn normalize_windows_path(path: &str) -> String {
    let normalized = path.replace('/', "\\");
    let without_verbatim_prefix = normalized
        .strip_prefix(r"\\?\UNC\")
        .map(|path| format!(r"\\{path}"))
        .or_else(|| normalized.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or(normalized);
    without_verbatim_prefix.to_lowercase()
}

fn json_protected_fields(config: &Map<String, Value>) -> Vec<String> {
    let mut fields = Vec::new();
    add_json_map_count(config, "env", "Environment variables", &mut fields);
    add_json_map_count(config, "headers", "HTTP headers", &mut fields);
    add_json_map_count(config, "http_headers", "HTTP headers", &mut fields);
    add_json_map_count(
        config,
        "env_http_headers",
        "Environment-backed headers",
        &mut fields,
    );
    if config.contains_key("envFile") || config.contains_key("env_file") {
        fields.push("Environment file".to_string());
    }
    if config.contains_key("bearer_token_env_var") {
        fields.push("Bearer token environment reference".to_string());
    }
    fields
}

fn add_json_map_count(
    config: &Map<String, Value>,
    key: &str,
    label: &str,
    fields: &mut Vec<String>,
) {
    if let Some(count) = config.get(key).and_then(Value::as_object).map(Map::len) {
        if count > 0 {
            fields.push(format!("{label} ({count})"));
        }
    }
}

fn toml_protected_fields(config: &toml::map::Map<String, toml::Value>) -> Vec<String> {
    let mut fields = Vec::new();
    add_toml_table_count(config, "env", "Environment variables", &mut fields);
    add_toml_table_count(config, "http_headers", "HTTP headers", &mut fields);
    add_toml_table_count(
        config,
        "env_http_headers",
        "Environment-backed headers",
        &mut fields,
    );
    if config.contains_key("env_vars") {
        fields.push("Forwarded environment variables".to_string());
    }
    if config.contains_key("bearer_token_env_var") {
        fields.push("Bearer token environment reference".to_string());
    }
    fields
}

fn add_toml_table_count(
    config: &toml::map::Map<String, toml::Value>,
    key: &str,
    label: &str,
    fields: &mut Vec<String>,
) {
    if let Some(count) = config
        .get(key)
        .and_then(toml::Value::as_table)
        .map(toml::map::Map::len)
    {
        if count > 0 {
            fields.push(format!("{label} ({count})"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_windows_path;

    #[test]
    fn normalizes_windows_verbatim_drive_and_unc_paths() {
        assert_eq!(
            normalize_windows_path(r"\\?\C:\Users\Ryan\.claude.json"),
            normalize_windows_path(r"C:\Users\Ryan\.claude.json")
        );
        assert_eq!(
            normalize_windows_path(r"\\?\UNC\server\share\SKILL.md"),
            normalize_windows_path(r"\\server\share\SKILL.md")
        );
    }
}
