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

pub struct CursorAdapter;

impl ClientAdapter for CursorAdapter {
    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::complete(ClientKind::Cursor)
    }

    fn discover(&self, context: &DiscoveryContext, snapshot: &mut InventorySnapshot) {
        self.discover_skills(context, snapshot);
        self.discover_user_configs(context, snapshot);
        self.discover_project_configs(context, snapshot);
    }
}

impl CursorAdapter {
    fn discover_skills(&self, context: &DiscoveryContext, snapshot: &mut InventorySnapshot) {
        let disabled = HashSet::new();
        let user_roots = [
            (".agents/skills", SourceKind::UserSkills, 400),
            (".cursor/skills", SourceKind::UserSkills, 300),
            (".claude/skills", SourceKind::LegacySkills, 200),
            (".codex/skills", SourceKind::LegacySkills, 100),
        ];
        for (relative_path, source_kind, source_priority) in user_roots {
            scan_skill_root(
                &context.home_dir.join(relative_path),
                ClientKind::Cursor,
                InventoryScope::User,
                source_kind,
                None,
                source_priority,
                true,
                &disabled,
                snapshot,
            );
        }
        let project_roots = [
            (".agents", SourceKind::ProjectSkills, 400),
            (".cursor", SourceKind::ProjectSkills, 300),
            (".claude", SourceKind::LegacySkills, 200),
            (".codex", SourceKind::LegacySkills, 100),
        ];
        for project_root in &context.project_roots {
            for (parent_name, source_kind, source_priority) in project_roots {
                for skills_root in discover_project_skill_roots(project_root, &[parent_name]) {
                    scan_skill_root(
                        &skills_root,
                        ClientKind::Cursor,
                        InventoryScope::Project,
                        source_kind,
                        Some(project_root),
                        source_priority,
                        true,
                        &disabled,
                        snapshot,
                    );
                }
            }
        }
    }

    fn discover_user_configs(&self, context: &DiscoveryContext, snapshot: &mut InventorySnapshot) {
        self.discover_mcp_file(
            &context.home_dir.join(".cursor/mcp.json"),
            InventoryScope::User,
            SourceKind::UserConfig,
            None,
            100,
            snapshot,
        );
        self.discover_hooks_file(
            &context.home_dir.join(".cursor/hooks.json"),
            InventoryScope::User,
            SourceKind::UserConfig,
            None,
            100,
            TrustState::NotApplicable,
            snapshot,
        );
    }

    fn discover_project_configs(
        &self,
        context: &DiscoveryContext,
        snapshot: &mut InventorySnapshot,
    ) {
        for project_root in &context.project_roots {
            self.discover_mcp_file(
                &project_root.join(".cursor/mcp.json"),
                InventoryScope::Project,
                SourceKind::ProjectConfig,
                Some(project_root),
                200,
                snapshot,
            );
            self.discover_hooks_file(
                &project_root.join(".cursor/hooks.json"),
                InventoryScope::Project,
                SourceKind::ProjectConfig,
                Some(project_root),
                200,
                TrustState::Unknown,
                snapshot,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn discover_mcp_file(
        &self,
        config_path: &Path,
        scope: InventoryScope,
        source_kind: SourceKind,
        project_path: Option<&Path>,
        source_priority: u16,
        snapshot: &mut InventorySnapshot,
    ) {
        let Some(config) = read_json(config_path, ClientKind::Cursor, snapshot) else {
            return;
        };
        let Some(servers) = config.get("mcpServers").and_then(Value::as_object) else {
            return;
        };
        push_json_mcps(
            servers,
            config_path,
            ClientKind::Cursor,
            scope,
            source_kind,
            project_path,
            source_priority,
            &HashSet::new(),
            None,
            trust_state_for_scope(scope),
            snapshot,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn discover_hooks_file(
        &self,
        config_path: &Path,
        scope: InventoryScope,
        source_kind: SourceKind,
        project_path: Option<&Path>,
        source_priority: u16,
        trust_state: TrustState,
        snapshot: &mut InventorySnapshot,
    ) {
        let Some(config) = read_json(config_path, ClientKind::Cursor, snapshot) else {
            return;
        };
        push_json_hooks(
            &config,
            config_path,
            ClientKind::Cursor,
            scope,
            source_kind,
            project_path,
            source_priority,
            true,
            trust_state,
            snapshot,
        );
    }
}

fn trust_state_for_scope(scope: InventoryScope) -> TrustState {
    match scope {
        InventoryScope::Project => TrustState::Unknown,
        InventoryScope::User | InventoryScope::Admin | InventoryScope::Legacy => {
            TrustState::NotApplicable
        }
    }
}
