use super::config::{push_json_mcps, read_json};
use super::hooks::push_codex_json_hooks;
use super::models::{
    ClientKind, DiscoveryContext, InventoryScope, InventorySnapshot, InventoryWarning, SourceKind,
    TrustState,
};
use super::skills::scan_skill_root;
use resolver::resolve_plugin_root;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

mod resolver;

pub fn discover_codex_plugins(
    context: &DiscoveryContext,
    config: Option<&toml::Value>,
    disabled_skills: &[PathBuf],
    hooks_enabled: bool,
    snapshot: &mut InventorySnapshot,
) {
    let Some(plugins) = config
        .and_then(|value| value.get("plugins"))
        .and_then(toml::Value::as_table)
    else {
        return;
    };
    for (plugin_id, state) in plugins {
        let enabled = state
            .get("enabled")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);
        let Some((plugin_name, marketplace_name)) = plugin_id.rsplit_once('@') else {
            push_plugin_warning(
                snapshot,
                &context.codex_home,
                "Skipped a configured plugin with an invalid identifier.",
            );
            continue;
        };
        let Some(plugin_root) =
            resolve_plugin_root(context, config, marketplace_name, plugin_name, snapshot)
        else {
            continue;
        };
        discover_plugin_components(
            &plugin_root,
            plugin_id,
            enabled,
            hooks_enabled,
            config,
            disabled_skills,
            snapshot,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn discover_plugin_components(
    plugin_root: &Path,
    plugin_id: &str,
    enabled: bool,
    hooks_enabled: bool,
    config: Option<&toml::Value>,
    disabled_skills: &[PathBuf],
    snapshot: &mut InventorySnapshot,
) {
    let manifest_path = [
        plugin_root.join(".codex-plugin/plugin.json"),
        plugin_root.join(".claude-plugin/plugin.json"),
    ]
    .into_iter()
    .find(|path| path.is_file());
    let manifest = manifest_path
        .as_ref()
        .and_then(|path| read_json(path, ClientKind::Codex, snapshot));

    let skills_root = component_path(manifest.as_ref(), "skills", plugin_root)
        .unwrap_or_else(|| plugin_root.join("skills"));
    scan_skill_root(
        &skills_root,
        ClientKind::Codex,
        InventoryScope::User,
        SourceKind::PluginSkills,
        None,
        150,
        true,
        enabled,
        TrustState::NotApplicable,
        disabled_skills,
        snapshot,
    );

    let mcp_path = component_path(manifest.as_ref(), "mcpServers", plugin_root)
        .unwrap_or_else(|| plugin_root.join(".mcp.json"));
    if let Some(mcp_config) = read_json(&mcp_path, ClientKind::Codex, snapshot) {
        if let Some(servers) = mcp_config.get("mcpServers").and_then(Value::as_object) {
            let record_start = snapshot.records.len();
            let disabled = if enabled {
                HashSet::new()
            } else {
                servers.keys().cloned().collect()
            };
            push_json_mcps(
                servers,
                &mcp_path,
                ClientKind::Codex,
                InventoryScope::User,
                SourceKind::PluginConfig,
                None,
                150,
                &disabled,
                None,
                &HashSet::new(),
                TrustState::NotApplicable,
                snapshot,
            );
            if !enabled {
                for record in &mut snapshot.records[record_start..] {
                    record.enabled = Some(false);
                    record.is_effective = Some(false);
                }
            }
        }
    }

    let hook_paths = component_paths(manifest.as_ref(), "hooks", plugin_root);
    let hook_paths = if hook_paths.is_empty() {
        vec![plugin_root.join("hooks/hooks.json")]
    } else {
        hook_paths
    };
    for hooks_path in hook_paths {
        let Some(hooks) = read_json(&hooks_path, ClientKind::Codex, snapshot) else {
            continue;
        };
        let relative = hooks_path
            .strip_prefix(plugin_root)
            .unwrap_or(&hooks_path)
            .to_string_lossy()
            .replace('\\', "/");
        let key_source = format!("{plugin_id}:{relative}");
        push_codex_json_hooks(
            &hooks,
            &hooks_path,
            InventoryScope::User,
            SourceKind::PluginConfig,
            None,
            150,
            enabled && hooks_enabled,
            TrustState::NotApplicable,
            config,
            &key_source,
            snapshot,
        );
    }
}

fn component_path(manifest: Option<&Value>, key: &str, plugin_root: &Path) -> Option<PathBuf> {
    let path = manifest?.get(key)?.as_str()?;
    Some(plugin_root.join(path))
}

fn component_paths(manifest: Option<&Value>, key: &str, plugin_root: &Path) -> Vec<PathBuf> {
    match manifest.and_then(|value| value.get(key)) {
        Some(Value::String(path)) => vec![plugin_root.join(path)],
        Some(Value::Array(paths)) => paths
            .iter()
            .filter_map(Value::as_str)
            .map(|path| plugin_root.join(path))
            .collect(),
        _ => Vec::new(),
    }
}

fn push_plugin_warning(snapshot: &mut InventorySnapshot, path: &Path, message: &str) {
    snapshot.warnings.push(InventoryWarning::new(
        ClientKind::Codex,
        path.display().to_string(),
        message,
    ));
}
