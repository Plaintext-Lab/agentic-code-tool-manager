use super::config::{push_json_mcps, read_json};
use super::hooks::push_json_hooks;
use super::models::{
    AdapterCapabilities, ClientKind, DiscoveryContext, InventoryScope, InventorySnapshot,
    SourceKind, TrustState,
};
use super::skills::{discover_project_skill_roots, scan_skill_root};
use super::ClientAdapter;
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

pub struct ClaudeAdapter;

impl ClientAdapter for ClaudeAdapter {
    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::complete(ClientKind::Claude)
    }

    fn discover(&self, context: &DiscoveryContext, snapshot: &mut InventorySnapshot) {
        self.discover_skills(context, snapshot);
        self.discover_user_state(context, snapshot);
        self.discover_project_configs(context, snapshot);
    }
}

impl ClaudeAdapter {
    fn discover_skills(&self, context: &DiscoveryContext, snapshot: &mut InventorySnapshot) {
        let disabled = HashSet::new();
        scan_skill_root(
            &context.home_dir.join(".claude/skills"),
            ClientKind::Claude,
            InventoryScope::User,
            SourceKind::UserSkills,
            None,
            200,
            false,
            &disabled,
            snapshot,
        );
        for project_root in &context.project_roots {
            for skills_root in discover_project_skill_roots(project_root, &[".claude"]) {
                scan_skill_root(
                    &skills_root,
                    ClientKind::Claude,
                    InventoryScope::Project,
                    SourceKind::ProjectSkills,
                    Some(project_root),
                    100,
                    false,
                    &disabled,
                    snapshot,
                );
            }
        }
    }

    fn discover_user_state(&self, context: &DiscoveryContext, snapshot: &mut InventorySnapshot) {
        let claude_json_path = context.home_dir.join(".claude.json");
        if let Some(config) = read_json(&claude_json_path, ClientKind::Claude, snapshot) {
            if let Some(servers) = config.get("mcpServers").and_then(Value::as_object) {
                push_json_mcps(
                    servers,
                    &claude_json_path,
                    ClientKind::Claude,
                    InventoryScope::User,
                    SourceKind::UserConfig,
                    None,
                    100,
                    &HashSet::new(),
                    snapshot,
                );
            }
            self.discover_local_mcps(&config, &claude_json_path, snapshot);
        }

        let settings_path = context.home_dir.join(".claude/settings.json");
        if let Some(settings) = read_json(&settings_path, ClientKind::Claude, snapshot) {
            let enabled = settings
                .get("disableAllHooks")
                .and_then(Value::as_bool)
                .map(|disabled| !disabled)
                .unwrap_or(true);
            push_json_hooks(
                &settings,
                &settings_path,
                ClientKind::Claude,
                InventoryScope::User,
                SourceKind::UserConfig,
                None,
                100,
                enabled,
                TrustState::NotApplicable,
                snapshot,
            );
        }
    }

    fn discover_local_mcps(
        &self,
        config: &Value,
        config_path: &Path,
        snapshot: &mut InventorySnapshot,
    ) {
        let Some(projects) = config.get("projects").and_then(Value::as_object) else {
            return;
        };
        for (project_path, project_config) in projects {
            let Some(project_config) = project_config.as_object() else {
                continue;
            };
            let Some(servers) = project_config.get("mcpServers").and_then(Value::as_object) else {
                continue;
            };
            let disabled = project_config
                .get("disabledMcpServers")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            push_json_mcps(
                servers,
                config_path,
                ClientKind::Claude,
                InventoryScope::Project,
                SourceKind::LocalConfig,
                Some(Path::new(project_path)),
                300,
                &disabled,
                snapshot,
            );
        }
    }

    fn discover_project_configs(
        &self,
        context: &DiscoveryContext,
        snapshot: &mut InventorySnapshot,
    ) {
        for project_root in &context.project_roots {
            let mcp_path = project_root.join(".mcp.json");
            if let Some(config) = read_json(&mcp_path, ClientKind::Claude, snapshot) {
                if let Some(servers) = config.get("mcpServers").and_then(Value::as_object) {
                    push_json_mcps(
                        servers,
                        &mcp_path,
                        ClientKind::Claude,
                        InventoryScope::Project,
                        SourceKind::ProjectConfig,
                        Some(project_root),
                        200,
                        &HashSet::new(),
                        snapshot,
                    );
                }
            }
            self.discover_project_hooks(project_root, "settings.json", 200, snapshot);
            self.discover_project_hooks(project_root, "settings.local.json", 300, snapshot);
        }
    }

    fn discover_project_hooks(
        &self,
        project_root: &Path,
        filename: &str,
        source_priority: u16,
        snapshot: &mut InventorySnapshot,
    ) {
        let settings_path = project_root.join(".claude").join(filename);
        let Some(settings) = read_json(&settings_path, ClientKind::Claude, snapshot) else {
            return;
        };
        let enabled = settings
            .get("disableAllHooks")
            .and_then(Value::as_bool)
            .map(|disabled| !disabled)
            .unwrap_or(true);
        push_json_hooks(
            &settings,
            &settings_path,
            ClientKind::Claude,
            InventoryScope::Project,
            if filename.ends_with("local.json") {
                SourceKind::LocalConfig
            } else {
                SourceKind::ProjectConfig
            },
            Some(project_root),
            source_priority,
            enabled,
            TrustState::NotApplicable,
            snapshot,
        );
    }
}
