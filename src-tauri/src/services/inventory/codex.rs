use super::codex_skill_config::codex_skill_config_match_count;
use super::config::{push_toml_mcps, read_json, read_toml};
use super::hooks::{push_codex_json_hooks, push_toml_hooks};
use super::models::{
    source_action_blocker, ActionBlockedReason, AdapterCapabilities, ClientKind, DiscoveryContext,
    InventoryActionCapabilities, InventoryRecord, InventoryScope, InventorySnapshot, SourceKind,
    TrustState,
};
use super::plugins::discover_codex_plugins;
use super::skills::{
    codex_disabled_skill_paths, codex_paths_match, codex_skill_config_is_editable,
    discover_project_skill_roots, scan_skill_root,
};
use super::ClientAdapter;
use std::path::{Path, PathBuf};

pub struct CodexAdapter;

impl ClientAdapter for CodexAdapter {
    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::complete(ClientKind::Codex)
    }

    fn discover(&self, context: &DiscoveryContext, snapshot: &mut InventorySnapshot) {
        let global_config_path = context.codex_home.join("config.toml");
        let global_config = read_toml(&global_config_path, ClientKind::Codex, snapshot);
        let disabled_skills = codex_disabled_skill_paths(global_config.as_ref());
        self.discover_skills(context, global_config.as_ref(), &disabled_skills, snapshot);

        let global_hooks_enabled = hooks_enabled(global_config.as_ref(), true);
        if let Some(config) = global_config.as_ref() {
            push_toml_mcps(
                config,
                &global_config_path,
                ClientKind::Codex,
                InventoryScope::User,
                SourceKind::UserConfig,
                None,
                100,
                TrustState::NotApplicable,
                snapshot,
            );
            push_toml_hooks(
                config,
                &global_config_path,
                InventoryScope::User,
                SourceKind::UserConfig,
                None,
                100,
                global_hooks_enabled,
                TrustState::NotApplicable,
                global_config.as_ref(),
                snapshot,
            );
        }
        self.discover_hooks_file(
            &context.codex_home.join("hooks.json"),
            InventoryScope::User,
            SourceKind::UserConfig,
            None,
            100,
            global_hooks_enabled,
            TrustState::NotApplicable,
            global_config.as_ref(),
            snapshot,
        );
        discover_codex_plugins(
            context,
            global_config.as_ref(),
            &disabled_skills,
            global_hooks_enabled,
            snapshot,
        );
        self.discover_project_configs(
            context,
            global_config.as_ref(),
            global_hooks_enabled,
            snapshot,
        );
        let skill_config_is_editable = codex_skill_config_is_editable(global_config.as_ref());
        let codex_skill_sources: Vec<(PathBuf, Option<PathBuf>, Option<PathBuf>)> = snapshot
            .records
            .iter()
            .filter(|record| {
                record.client == ClientKind::Codex
                    && record.item_type == super::models::InventoryItemType::Skill
            })
            .map(|record| {
                (
                    PathBuf::from(&record.original_path),
                    record.resolved_path.as_deref().map(PathBuf::from),
                    codex_skill_relative_path(context, record),
                )
            })
            .collect();
        for record in snapshot.records.iter_mut().filter(|record| {
            record.client == ClientKind::Codex
                && record.item_type == super::models::InventoryItemType::Skill
        }) {
            let record_path = Path::new(&record.original_path);
            let record_resolved_path = record.resolved_path.as_deref().map(Path::new);
            let record_relative_path = codex_skill_relative_path(context, record);
            let matching_source_count = codex_skill_sources
                .iter()
                .filter(
                    |(candidate_path, candidate_resolved_path, candidate_relative_path)| {
                        codex_paths_match(candidate_path, record_path)
                            || candidate_relative_path.as_ref() == record_relative_path.as_ref()
                                && candidate_relative_path.is_some()
                                && candidate_resolved_path.as_deref().is_some_and(|candidate| {
                                    record_resolved_path
                                        .is_some_and(|record| codex_paths_match(candidate, record))
                                })
                    },
                )
                .count();
            if !skill_config_is_editable
                || codex_skill_config_match_count(global_config.as_ref(), record) > 1
                || matching_source_count > 1
            {
                record.restrict_actions(ActionBlockedReason::MalformedSource);
            }
        }
    }

    fn action_capabilities(
        &self,
        record: &InventoryRecord,
        source_revision: String,
    ) -> InventoryActionCapabilities {
        if let Some(reason) = source_action_blocker(record) {
            return InventoryActionCapabilities::blocked(reason, Some(source_revision));
        }
        if record.item_type == super::models::InventoryItemType::Skill && !cfg!(target_os = "macos")
        {
            return InventoryActionCapabilities::blocked(
                ActionBlockedReason::UnsupportedByClient,
                Some(source_revision),
            );
        }
        if matches!(
            record.source_kind,
            SourceKind::UserConfig
                | SourceKind::ProjectConfig
                | SourceKind::UserSkills
                | SourceKind::ProjectSkills
                | SourceKind::LegacySkills
        ) {
            if record.item_type == super::models::InventoryItemType::Hook && record.approval_pending
            {
                return InventoryActionCapabilities::pending_approval(
                    true,
                    super::models::ReloadGuidance::RestartClient,
                    source_revision,
                );
            }
            return InventoryActionCapabilities::stateful(
                record.enabled,
                true,
                super::models::ReloadGuidance::RestartClient,
                source_revision,
            );
        }
        InventoryActionCapabilities::blocked(
            ActionBlockedReason::UnsupportedByClient,
            Some(source_revision),
        )
    }

    fn action_revision_sources(
        &self,
        context: &DiscoveryContext,
        record: &InventoryRecord,
    ) -> Vec<String> {
        let mut sources = vec![record.source_path.clone()];
        if source_action_blocker(record).is_some() {
            return sources;
        }
        let global_config = context.codex_home.join("config.toml").display().to_string();
        match record.item_type {
            super::models::InventoryItemType::Skill => sources.push(global_config),
            super::models::InventoryItemType::Hook => {
                sources.push(global_config);
                if let Some(project_path) = record.project_path.as_ref() {
                    sources.push(
                        Path::new(project_path)
                            .join(".codex/config.toml")
                            .display()
                            .to_string(),
                    );
                }
            }
            super::models::InventoryItemType::Mcp => {}
        }
        sources
    }
}

fn codex_skill_relative_path(
    context: &DiscoveryContext,
    record: &InventoryRecord,
) -> Option<PathBuf> {
    let root = match (record.source_kind, record.scope) {
        (SourceKind::UserSkills, InventoryScope::User) => context.home_dir.join(".agents/skills"),
        (SourceKind::ProjectSkills, InventoryScope::Project) => {
            Path::new(record.project_path.as_deref()?).join(".agents/skills")
        }
        (SourceKind::LegacySkills, InventoryScope::Legacy) => context.codex_home.join("skills"),
        (SourceKind::LegacySkills, InventoryScope::Project) => {
            Path::new(record.project_path.as_deref()?).join(".codex/skills")
        }
        (SourceKind::AdminSkills, InventoryScope::Admin) => PathBuf::from("/etc/codex/skills"),
        _ => return None,
    };
    Path::new(&record.original_path)
        .strip_prefix(root)
        .ok()
        .map(PathBuf::from)
}

impl CodexAdapter {
    fn discover_skills(
        &self,
        context: &DiscoveryContext,
        global_config: Option<&toml::Value>,
        disabled_skills: &[PathBuf],
        snapshot: &mut InventorySnapshot,
    ) {
        scan_skill_root(
            &context.home_dir.join(".agents/skills"),
            ClientKind::Codex,
            InventoryScope::User,
            SourceKind::UserSkills,
            None,
            100,
            true,
            true,
            TrustState::NotApplicable,
            disabled_skills,
            snapshot,
        );
        scan_skill_root(
            &context.codex_home.join("skills"),
            ClientKind::Codex,
            InventoryScope::Legacy,
            SourceKind::LegacySkills,
            None,
            50,
            true,
            true,
            TrustState::NotApplicable,
            disabled_skills,
            snapshot,
        );
        scan_skill_root(
            Path::new("/etc/codex/skills"),
            ClientKind::Codex,
            InventoryScope::Admin,
            SourceKind::AdminSkills,
            None,
            300,
            true,
            true,
            TrustState::NotApplicable,
            disabled_skills,
            snapshot,
        );
        for project_root in &context.project_roots {
            let trust_state = project_trust_state(global_config, project_root);
            for skills_root in discover_project_skill_roots(project_root, &[".agents"]) {
                scan_skill_root(
                    &skills_root,
                    ClientKind::Codex,
                    InventoryScope::Project,
                    SourceKind::ProjectSkills,
                    Some(project_root),
                    200,
                    true,
                    true,
                    trust_state,
                    disabled_skills,
                    snapshot,
                );
            }
            for skills_root in discover_project_skill_roots(project_root, &[".codex"]) {
                scan_skill_root(
                    &skills_root,
                    ClientKind::Codex,
                    InventoryScope::Project,
                    SourceKind::LegacySkills,
                    Some(project_root),
                    50,
                    true,
                    true,
                    trust_state,
                    disabled_skills,
                    snapshot,
                );
            }
        }
    }

    fn discover_project_configs(
        &self,
        context: &DiscoveryContext,
        global_config: Option<&toml::Value>,
        global_hooks_enabled: bool,
        snapshot: &mut InventorySnapshot,
    ) {
        for project_root in &context.project_roots {
            let trust_state = project_trust_state(global_config, project_root);
            let config_path = project_root.join(".codex/config.toml");
            let config = read_toml(&config_path, ClientKind::Codex, snapshot);
            let project_hooks_enabled = hooks_enabled(config.as_ref(), global_hooks_enabled);
            if let Some(config) = config.as_ref() {
                push_toml_mcps(
                    config,
                    &config_path,
                    ClientKind::Codex,
                    InventoryScope::Project,
                    SourceKind::ProjectConfig,
                    Some(project_root),
                    200,
                    trust_state,
                    snapshot,
                );
                push_toml_hooks(
                    config,
                    &config_path,
                    InventoryScope::Project,
                    SourceKind::ProjectConfig,
                    Some(project_root),
                    200,
                    project_hooks_enabled,
                    trust_state,
                    global_config,
                    snapshot,
                );
            }
            self.discover_hooks_file(
                &project_root.join(".codex/hooks.json"),
                InventoryScope::Project,
                SourceKind::ProjectConfig,
                Some(project_root),
                200,
                project_hooks_enabled,
                trust_state,
                global_config,
                snapshot,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn discover_hooks_file(
        &self,
        hooks_path: &Path,
        scope: InventoryScope,
        source_kind: SourceKind,
        project_path: Option<&Path>,
        source_priority: u16,
        enabled: bool,
        trust_state: TrustState,
        state_config: Option<&toml::Value>,
        snapshot: &mut InventorySnapshot,
    ) {
        let Some(config) = read_json(hooks_path, ClientKind::Codex, snapshot) else {
            return;
        };
        push_codex_json_hooks(
            &config,
            hooks_path,
            scope,
            source_kind,
            project_path,
            source_priority,
            enabled,
            trust_state,
            state_config,
            &hooks_path.display().to_string(),
            snapshot,
        );
    }
}

fn hooks_enabled(config: Option<&toml::Value>, fallback: bool) -> bool {
    config
        .and_then(|value| value.get("features"))
        .and_then(toml::Value::as_table)
        .and_then(|features| {
            features
                .get("hooks")
                .or_else(|| features.get("codex_hooks"))
        })
        .and_then(toml::Value::as_bool)
        .unwrap_or(fallback)
}

fn project_trust_state(config: Option<&toml::Value>, project_root: &Path) -> TrustState {
    let Some(projects) = config
        .and_then(|value| value.get("projects"))
        .and_then(toml::Value::as_table)
    else {
        return TrustState::Unknown;
    };
    let direct_key = project_root.display().to_string();
    let project = projects.get(&direct_key).or_else(|| {
        let canonical_root = std::fs::canonicalize(project_root).ok()?;
        projects.iter().find_map(|(path, project)| {
            let candidate = std::fs::canonicalize(path).ok()?;
            (candidate == canonical_root).then_some(project)
        })
    });
    match project
        .and_then(|value| value.get("trust_level"))
        .and_then(toml::Value::as_str)
    {
        Some("trusted") => TrustState::Trusted,
        Some("untrusted") => TrustState::Untrusted,
        _ => TrustState::Unknown,
    }
}
