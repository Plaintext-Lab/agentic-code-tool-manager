use super::claude_policy::ClaudeManagedPolicy;
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
        let managed_settings = read_json(
            &context.claude_managed_settings_path,
            ClientKind::Claude,
            snapshot,
        );
        let managed_mcp = read_json(
            &context.claude_managed_mcp_path,
            ClientKind::Claude,
            snapshot,
        );
        let policy = ClaudeManagedPolicy::new(
            managed_settings.as_ref(),
            context.claude_managed_mcp_path.exists(),
        );
        self.discover_skills(context, snapshot);
        self.discover_managed_state(
            context,
            managed_settings.as_ref(),
            managed_mcp.as_ref(),
            &policy,
            snapshot,
        );
        let (user_state, user_hooks_enabled) = self.discover_user_state(context, &policy, snapshot);
        self.discover_project_configs(
            context,
            user_state.as_ref(),
            user_hooks_enabled,
            &policy,
            snapshot,
        );
        if policy.managed_mcp_exclusive {
            for record in &mut snapshot.records {
                if record.client == ClientKind::Claude
                    && record.item_type == super::models::InventoryItemType::Mcp
                    && record.source_kind != SourceKind::ManagedConfig
                {
                    record.is_effective = Some(false);
                }
            }
        }
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
        policy: &ClaudeManagedPolicy,
        snapshot: &mut InventorySnapshot,
    ) -> (Option<Value>, bool) {
        let claude_json_path = context.home_dir.join(".claude.json");
        let config = read_json(&claude_json_path, ClientKind::Claude, snapshot);
        if let Some(config) = config.as_ref() {
            if let Some(servers) = config.get("mcpServers").and_then(Value::as_object) {
                let policy_blocked = policy.blocked_names(servers);
                push_json_mcps(
                    servers,
                    &claude_json_path,
                    ClientKind::Claude,
                    InventoryScope::User,
                    SourceKind::UserConfig,
                    None,
                    100,
                    &HashSet::new(),
                    None,
                    &policy_blocked,
                    TrustState::NotApplicable,
                    snapshot,
                );
            }
            self.discover_local_mcps(config, &claude_json_path, policy, snapshot);
        }

        let settings_path = context.home_dir.join(".claude/settings.json");
        let settings = read_json(&settings_path, ClientKind::Claude, snapshot);
        let hooks_enabled =
            hook_setting(settings.as_ref()).unwrap_or(true) && !policy.allow_managed_hooks_only;
        if let Some(settings) = settings.as_ref() {
            push_json_hooks(
                settings,
                &settings_path,
                ClientKind::Claude,
                InventoryScope::User,
                SourceKind::UserConfig,
                None,
                100,
                hooks_enabled,
                TrustState::NotApplicable,
                snapshot,
            );
        }
        (config, hooks_enabled)
    }

    fn discover_local_mcps(
        &self,
        config: &Value,
        config_path: &Path,
        policy: &ClaudeManagedPolicy,
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
            let policy_blocked = policy.blocked_names(servers);
            push_json_mcps(
                servers,
                config_path,
                ClientKind::Claude,
                InventoryScope::Project,
                SourceKind::LocalConfig,
                Some(Path::new(project_path)),
                300,
                &disabled,
                None,
                &policy_blocked,
                trust_state,
                snapshot,
            );
        }
    }

    fn discover_project_configs(
        &self,
        context: &DiscoveryContext,
        user_state: Option<&Value>,
        user_hooks_enabled: bool,
        policy: &ClaudeManagedPolicy,
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
                    let approved = if project_state
                        .and_then(|state| state.get("enableAllProjectMcpServers"))
                        .and_then(Value::as_bool)
                        == Some(true)
                    {
                        None
                    } else {
                        Some(
                            project_state
                                .map(|state| disabled_names(state, "enabledMcpjsonServers"))
                                .unwrap_or_default(),
                        )
                    };
                    let policy_blocked = policy.blocked_names(servers);
                    push_json_mcps(
                        servers,
                        &mcp_path,
                        ClientKind::Claude,
                        InventoryScope::Project,
                        SourceKind::ProjectConfig,
                        Some(project_root),
                        200,
                        &disabled,
                        approved.as_ref(),
                        &policy_blocked,
                        trust_state,
                        snapshot,
                    );
                }
            }
            let settings_path = project_root.join(".claude/settings.json");
            let local_settings_path = project_root.join(".claude/settings.local.json");
            let settings = read_json(&settings_path, ClientKind::Claude, snapshot);
            let local_settings = read_json(&local_settings_path, ClientKind::Claude, snapshot);
            let hooks_enabled = hook_setting(local_settings.as_ref())
                .or_else(|| hook_setting(settings.as_ref()))
                .unwrap_or(user_hooks_enabled)
                && !policy.allow_managed_hooks_only;
            if let Some(settings) = settings.as_ref() {
                self.push_project_hooks(
                    project_root,
                    &settings_path,
                    settings,
                    SourceKind::ProjectConfig,
                    200,
                    hooks_enabled,
                    trust_state,
                    snapshot,
                );
            }
            if let Some(settings) = local_settings.as_ref() {
                self.push_project_hooks(
                    project_root,
                    &local_settings_path,
                    settings,
                    SourceKind::LocalConfig,
                    300,
                    hooks_enabled,
                    trust_state,
                    snapshot,
                );
            }
        }
    }

    fn discover_managed_state(
        &self,
        context: &DiscoveryContext,
        managed_settings: Option<&Value>,
        managed_mcp: Option<&Value>,
        policy: &ClaudeManagedPolicy,
        snapshot: &mut InventorySnapshot,
    ) {
        if let Some(settings) = managed_settings {
            push_json_hooks(
                settings,
                &context.claude_managed_settings_path,
                ClientKind::Claude,
                InventoryScope::Admin,
                SourceKind::ManagedConfig,
                None,
                400,
                hook_setting(Some(settings)).unwrap_or(true),
                TrustState::NotApplicable,
                snapshot,
            );
        }
        if let Some(servers) = managed_mcp
            .and_then(|config| config.get("mcpServers"))
            .and_then(Value::as_object)
        {
            let policy_blocked = policy.blocked_names(servers);
            push_json_mcps(
                servers,
                &context.claude_managed_mcp_path,
                ClientKind::Claude,
                InventoryScope::Admin,
                SourceKind::ManagedConfig,
                None,
                400,
                &HashSet::new(),
                None,
                &policy_blocked,
                TrustState::NotApplicable,
                snapshot,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_project_hooks(
        &self,
        project_root: &Path,
        settings_path: &Path,
        settings: &Value,
        source_kind: SourceKind,
        source_priority: u16,
        enabled: bool,
        trust_state: TrustState,
        snapshot: &mut InventorySnapshot,
    ) {
        push_json_hooks(
            settings,
            settings_path,
            ClientKind::Claude,
            InventoryScope::Project,
            source_kind,
            Some(project_root),
            source_priority,
            enabled,
            trust_state,
            snapshot,
        );
    }
}

fn hook_setting(settings: Option<&Value>) -> Option<bool> {
    settings?
        .get("disableAllHooks")
        .and_then(Value::as_bool)
        .map(|disabled| !disabled)
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
