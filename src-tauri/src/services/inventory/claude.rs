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
        let user_state = self.discover_user_state(context, snapshot);
        self.discover_project_configs(context, user_state.as_ref(), snapshot);
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

    fn discover_user_state(
        &self,
        context: &DiscoveryContext,
        snapshot: &mut InventorySnapshot,
    ) -> Option<Value> {
        let claude_json_path = context.home_dir.join(".claude.json");
        let config = read_json(&claude_json_path, ClientKind::Claude, snapshot);
        if let Some(config) = config.as_ref() {
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
                    TrustState::NotApplicable,
                    snapshot,
                );
            }
            self.discover_local_mcps(config, &claude_json_path, snapshot);
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
        config
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
            let trust_state = project_trust_state(project_config);
            push_json_mcps(
                servers,
                config_path,
                ClientKind::Claude,
                InventoryScope::Project,
                SourceKind::LocalConfig,
                Some(Path::new(project_path)),
                300,
                &disabled,
                trust_state,
                snapshot,
            );
        }
    }

    fn discover_project_configs(
        &self,
        context: &DiscoveryContext,
        user_state: Option<&Value>,
        snapshot: &mut InventorySnapshot,
    ) {
        for project_root in &context.project_roots {
            let project_state = find_project_state(user_state, project_root);
            let trust_state = project_state
                .map(project_trust_state)
                .unwrap_or(TrustState::Unknown);
            let mcp_path = project_root.join(".mcp.json");
            if let Some(config) = read_json(&mcp_path, ClientKind::Claude, snapshot) {
                if let Some(servers) = config.get("mcpServers").and_then(Value::as_object) {
                    let disabled = project_state
                        .map(|state| disabled_names(state, "disabledMcpjsonServers"))
                        .unwrap_or_default();
                    push_json_mcps(
                        servers,
                        &mcp_path,
                        ClientKind::Claude,
                        InventoryScope::Project,
                        SourceKind::ProjectConfig,
                        Some(project_root),
                        200,
                        &disabled,
                        trust_state,
                        snapshot,
                    );
                }
            }
            self.discover_project_hooks(project_root, "settings.json", 200, trust_state, snapshot);
            self.discover_project_hooks(
                project_root,
                "settings.local.json",
                300,
                trust_state,
                snapshot,
            );
        }
    }

    fn discover_project_hooks(
        &self,
        project_root: &Path,
        filename: &str,
        source_priority: u16,
        trust_state: TrustState,
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
            trust_state,
            snapshot,
        );
    }
}

fn find_project_state<'a>(
    user_state: Option<&'a Value>,
    project_root: &Path,
) -> Option<&'a serde_json::Map<String, Value>> {
    let projects = user_state?.get("projects")?.as_object()?;
    let direct_key = project_root.display().to_string();
    if let Some(project) = projects.get(&direct_key).and_then(Value::as_object) {
        return Some(project);
    }
    projects.iter().find_map(|(path, project)| {
        let candidate = std::fs::canonicalize(path).ok()?;
        (candidate == project_root)
            .then(|| project.as_object())
            .flatten()
    })
}

fn project_trust_state(project: &serde_json::Map<String, Value>) -> TrustState {
    match project
        .get("hasTrustDialogAccepted")
        .and_then(Value::as_bool)
    {
        Some(true) => TrustState::Trusted,
        Some(false) => TrustState::Untrusted,
        None => TrustState::Unknown,
    }
}

fn disabled_names(project: &serde_json::Map<String, Value>, key: &str) -> HashSet<String> {
    project
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}
