use super::codex::CodexAdapter;
use super::models::{
    ActionBlockedReason, ClientKind, InventoryItemType, InventoryRecord, InventoryScope,
    InventorySnapshot, InventoryWarning, ReloadGuidance, SourceKind, TrustState,
};
use super::{
    codex_home_path, discover_inventory, discover_inventory_with_codex_home,
    discover_inventory_with_paths, warning_source_restriction, ClientAdapter,
};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const CLAUDE_CONFIG: &str = include_str!("fixtures/claude.json");
const CLAUDE_HOOKS: &str = include_str!("fixtures/claude_settings.json");
const CODEX_CONFIG: &str = include_str!("fixtures/codex_config.toml");
const CURSOR_MCP: &str = include_str!("fixtures/cursor_mcp.json");
const CURSOR_HOOKS: &str = include_str!("fixtures/cursor_hooks.json");
const SKILL: &str = include_str!("fixtures/skill.md");

#[test]
fn discovers_all_clients_and_never_serializes_secret_values() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let project = fixture.path().join("project");
    write(&home.join(".claude.json"), CLAUDE_CONFIG);
    write(&home.join(".claude/settings.json"), CLAUDE_HOOKS);
    write(&home.join(".codex/config.toml"), CODEX_CONFIG);
    write(&home.join(".cursor/mcp.json"), CURSOR_MCP);
    write(&home.join(".cursor/hooks.json"), CURSOR_HOOKS);
    write_skill(&home.join(".claude/skills/shared-skill/SKILL.md"));
    write_skill(&home.join(".agents/skills/shared-skill/SKILL.md"));
    write_skill(&home.join(".cursor/skills/shared-skill/SKILL.md"));

    write(&project.join(".mcp.json"), CURSOR_MCP);
    write(&project.join(".claude/settings.json"), CLAUDE_HOOKS);
    write(&project.join(".codex/config.toml"), CODEX_CONFIG);
    write(&project.join(".codex/hooks.json"), CLAUDE_HOOKS);
    write(&project.join(".cursor/mcp.json"), CURSOR_MCP);
    write(&project.join(".cursor/hooks.json"), CURSOR_HOOKS);
    write_skill(&project.join(".claude/skills/shared-skill/SKILL.md"));
    write_skill(&project.join(".agents/skills/shared-skill/SKILL.md"));
    write_skill(&project.join(".cursor/skills/shared-skill/SKILL.md"));

    let snapshot = discover_inventory(home, vec![project]);

    for client in [ClientKind::Claude, ClientKind::Codex, ClientKind::Cursor] {
        for item_type in [
            InventoryItemType::Skill,
            InventoryItemType::Mcp,
            InventoryItemType::Hook,
        ] {
            assert!(
                snapshot
                    .records
                    .iter()
                    .any(|record| record.client == client && record.item_type == item_type),
                "missing {client:?} {item_type:?} record"
            );
        }
    }
    let duplicate_skills = snapshot
        .records
        .iter()
        .filter(|record| {
            record.item_type == InventoryItemType::Skill && record.name == "shared-skill"
        })
        .count();
    assert!(
        duplicate_skills >= 6,
        "source-specific duplicates were lost"
    );
    assert!(snapshot
        .records
        .iter()
        .any(|record| { record.name == "local-server" && record.enabled == Some(false) }));
    assert!(snapshot
        .records
        .iter()
        .any(|record| !record.protected_fields.is_empty()));
    assert!(snapshot
        .records
        .iter()
        .all(|record| record.action_capabilities.source_revision.is_some()));

    let serialized = serde_json::to_string(&snapshot).unwrap();
    for secret in [
        "CLAUDE_SECRET_VALUE",
        "CLAUDE_URL_SECRET",
        "CLAUDE_HEADER_SECRET",
        "CLAUDE_HOOK_SECRET",
        "CODEX_SECRET_VALUE",
        "CODEX_HOOK_SECRET",
        "CURSOR_URL_SECRET",
        "CURSOR_HEADER_SECRET",
        "CURSOR_HOOK_SECRET",
    ] {
        assert!(
            !serialized.contains(secret),
            "serialized secret value: {secret}"
        );
    }
}

#[test]
fn reports_normalized_action_capabilities_for_each_client_boundary() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let managed_settings = fixture.path().join("managed-settings-missing.json");
    let managed_mcp = fixture.path().join("managed-mcp.json");
    let project = fixture.path().join("project");
    write(
        &home.join(".claude.json"),
        r#"{"mcpServers":{"claude-server":{"command":"claude-secret-command"},"disabled-policy-server":{"command":"disabled-policy-secret","disabled":true}}}"#,
    );
    write(
        &home.join(".codex/config.toml"),
        "[mcp_servers.codex-server]\ncommand = 'codex-secret-command'\n",
    );
    write(
        &home.join(".cursor/mcp.json"),
        r#"{"mcpServers":{"cursor-server":{"command":"cursor-secret-command"}}}"#,
    );
    write(
        &managed_mcp,
        r#"{"mcpServers":{"managed-server":{"command":"managed-secret-command"}}}"#,
    );
    write(
        &project.join(".mcp.json"),
        r#"{"mcpServers":{"project-policy-server":{"command":"project-policy-secret"}}}"#,
    );

    let snapshot = discover_inventory_with_paths(
        home.clone(),
        home.join(".codex"),
        managed_settings.clone(),
        fixture.path().join("managed-mcp-missing.json"),
        vec![project.clone()],
    );
    let record = |name: &str| {
        snapshot
            .records
            .iter()
            .find(|record| record.name == name)
            .expect("fixture record should be discovered")
    };

    for name in ["claude-server", "codex-server"] {
        let actions = &record(name).action_capabilities;
        assert!(!actions.enable.available);
        assert_eq!(
            actions.enable.blocked_reason,
            Some(ActionBlockedReason::AlreadyEnabled)
        );
        assert!(actions.disable.available);
        assert_eq!(actions.disable.blocked_reason, None);
        assert!(actions.confirmation_required);
        assert_eq!(actions.reload_guidance, ReloadGuidance::RestartClient);
        assert!(actions
            .source_revision
            .as_deref()
            .is_some_and(|revision| revision.starts_with("sha256:")));
    }
    let first_codex_revision = record("codex-server")
        .action_capabilities
        .source_revision
        .clone();
    write(
        &home.join(".codex/config.toml"),
        "[mcp_servers.codex-server]\ncommand = 'changed-codex-command'\n",
    );
    let changed_snapshot = discover_inventory(home.clone(), Vec::new());
    let changed_codex_revision = changed_snapshot
        .records
        .iter()
        .find(|record| record.name == "codex-server")
        .expect("changed Codex fixture should be discovered")
        .action_capabilities
        .source_revision
        .clone();
    assert_ne!(first_codex_revision, changed_codex_revision);

    let cursor_actions = &record("cursor-server").action_capabilities;
    assert!(!cursor_actions.enable.available);
    assert!(!cursor_actions.disable.available);
    assert_eq!(
        cursor_actions.enable.blocked_reason,
        Some(ActionBlockedReason::UnsupportedByClient)
    );
    assert_eq!(
        cursor_actions.disable.blocked_reason,
        Some(ActionBlockedReason::UnsupportedByClient)
    );

    let managed_snapshot = discover_inventory_with_paths(
        home.clone(),
        home.join(".codex"),
        managed_settings,
        managed_mcp,
        vec![project],
    );
    let managed_actions = &managed_snapshot
        .records
        .iter()
        .find(|record| record.name == "managed-server")
        .expect("managed fixture record should be discovered")
        .action_capabilities;
    assert!(!managed_actions.enable.available);
    assert!(!managed_actions.disable.available);
    assert_eq!(
        managed_actions.enable.blocked_reason,
        Some(ActionBlockedReason::ManagedSource)
    );
    for name in [
        "claude-server",
        "disabled-policy-server",
        "project-policy-server",
    ] {
        let policy_controlled = managed_snapshot
            .records
            .iter()
            .find(|record| record.name == name)
            .expect("managed policy should leave the source record visible");
        assert_eq!(
            policy_controlled.action_capabilities.enable.blocked_reason,
            Some(ActionBlockedReason::PolicyControlled)
        );
        assert_eq!(
            policy_controlled.action_capabilities.disable.blocked_reason,
            Some(ActionBlockedReason::PolicyControlled)
        );
    }

    let serialized = format!(
        "{}{}",
        serde_json::to_string(&snapshot).unwrap(),
        serde_json::to_string(&managed_snapshot).unwrap()
    );
    for protected_value in [
        "claude-secret-command",
        "codex-secret-command",
        "cursor-secret-command",
        "managed-secret-command",
        "changed-codex-command",
        "disabled-policy-secret",
        "project-policy-secret",
    ] {
        assert!(!serialized.contains(protected_value));
    }
}

#[test]
fn codex_boundary_blocks_administrator_and_broken_link_sources() {
    let mut administrator = InventoryRecord::new(
        ClientKind::Codex,
        InventoryItemType::Skill,
        "administrator-skill".to_string(),
        InventoryScope::Admin,
        SourceKind::AdminSkills,
        "/etc/codex/skills/administrator-skill/SKILL.md".to_string(),
        None,
        0,
        300,
    );
    administrator.enabled = Some(true);
    administrator.is_effective = Some(true);
    let administrator_actions = CodexAdapter
        .action_capabilities(&administrator, "sha256:administrator-fixture".to_string());
    assert_eq!(
        administrator_actions.disable.blocked_reason,
        Some(ActionBlockedReason::AdministratorSource)
    );

    let mut broken_link = InventoryRecord::new(
        ClientKind::Codex,
        InventoryItemType::Skill,
        "broken-link".to_string(),
        InventoryScope::User,
        SourceKind::UserSkills,
        "/tmp/broken-link/SKILL.md".to_string(),
        None,
        0,
        100,
    );
    broken_link.enabled = Some(true);
    broken_link.is_effective = Some(true);
    broken_link.is_symlink = true;
    let broken_link_actions =
        CodexAdapter.action_capabilities(&broken_link, "sha256:broken-link".to_string());
    assert_eq!(
        broken_link_actions.disable.blocked_reason,
        Some(ActionBlockedReason::BrokenSymlink)
    );

    let mut missing_revision = InventoryRecord::new(
        ClientKind::Codex,
        InventoryItemType::Mcp,
        "missing-revision".to_string(),
        InventoryScope::User,
        SourceKind::UserConfig,
        "/tmp/config.toml".to_string(),
        None,
        0,
        100,
    );
    missing_revision.enabled = Some(true);
    missing_revision.is_effective = Some(true);
    missing_revision.restrict_actions(ActionBlockedReason::StateUnavailable);
    let missing_revision_actions =
        CodexAdapter.action_capabilities(&missing_revision, "sha256:unobserved-source".to_string());
    assert_eq!(
        missing_revision_actions.disable.blocked_reason,
        Some(ActionBlockedReason::StateUnavailable)
    );
}

#[test]
fn reports_malformed_and_plugin_owned_records_as_read_only() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let marketplace = fixture.path().join("marketplace");
    let plugin = marketplace.join("plugins/demo");
    write(
        &home.join(".agents/skills/malformed/SKILL.md"),
        "---\nname: malformed\ndescription: missing delimiter\n",
    );
    write(
        &marketplace.join(".agents/plugins/marketplace.json"),
        r#"{"name":"local","plugins":[{"name":"demo","source":"./plugins/demo"}]}"#,
    );
    write(
        &plugin.join(".codex-plugin/plugin.json"),
        r#"{"name":"demo","skills":"./skills/"}"#,
    );
    write_skill(&plugin.join("skills/plugin-skill/SKILL.md"));
    write(
        &home.join(".codex/config.toml"),
        &format!(
            "[marketplaces.local]\nsource_type = 'local'\nsource = '{}'\n\n[plugins.'demo@local']\nenabled = true\n",
            marketplace.display()
        ),
    );

    let snapshot = discover_inventory(home, Vec::new());
    let malformed = snapshot
        .records
        .iter()
        .find(|record| record.name == "malformed")
        .expect("malformed skill should remain visible");
    let plugin_skill = snapshot
        .records
        .iter()
        .find(|record| record.source_kind == SourceKind::PluginSkills)
        .expect("plugin skill should be discovered");

    assert_eq!(
        malformed.action_capabilities.disable.blocked_reason,
        Some(ActionBlockedReason::MalformedSource)
    );
    assert_eq!(
        plugin_skill.action_capabilities.disable.blocked_reason,
        Some(ActionBlockedReason::PluginOwnedSource)
    );
}

#[test]
fn parse_warnings_do_not_echo_invalid_configuration_contents() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    write(
        &home.join(".claude.json"),
        "{\"mcpServers\": {\"token\": \"TOP_SECRET_CLAUDE\"}",
    );
    write(
        &home.join(".codex/config.toml"),
        "token = \"TOP_SECRET_CODEX\"\n[broken",
    );
    write(
        &home.join(".cursor/mcp.json"),
        "{\"token\": \"TOP_SECRET_CURSOR\"",
    );

    let snapshot = discover_inventory(home, Vec::new());
    assert_eq!(snapshot.warnings.len(), 3);
    let serialized = serde_json::to_string(&snapshot).unwrap();
    assert!(!serialized.contains("TOP_SECRET_CLAUDE"));
    assert!(!serialized.contains("TOP_SECRET_CODEX"));
    assert!(!serialized.contains("TOP_SECRET_CURSOR"));
}

#[test]
fn disabled_codex_skill_is_reported_without_deleting_it() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let skill_path = home.join(".agents/skills/disabled-skill/SKILL.md");
    write(
        &skill_path,
        "---\nname: disabled-skill\ndescription: Disabled fixture\n---\n",
    );
    write(
        &home.join(".codex/config.toml"),
        &format!(
            "[[skills.config]]\npath = '{}'\nenabled = false\n",
            skill_path.display()
        ),
    );

    let snapshot = discover_inventory(home, Vec::new());
    let skill = snapshot
        .records
        .iter()
        .find(|record| record.client == ClientKind::Codex && record.name == "disabled-skill")
        .expect("disabled skill should remain visible");
    assert_eq!(skill.enabled, Some(false));
    assert!(skill_path.exists());
}

#[test]
fn action_revisions_cover_native_state_owners() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let project = fixture.path().join("project");
    let skill_path = home.join(".agents/skills/stateful-skill/SKILL.md");
    write_skill(&skill_path);
    write(
        &home.join(".codex/hooks.json"),
        r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"stateful-hook"}]}]}}"#,
    );
    write(
        &project.join(".mcp.json"),
        r#"{"mcpServers":{"stateful-project":{"command":"project-server"}}}"#,
    );
    write(
        &home.join(".claude.json"),
        &format!(
            r#"{{"projects":{{"{}":{{"hasTrustDialogAccepted":true,"enableAllProjectMcpServers":true,"disabledMcpjsonServers":[]}}}}}}"#,
            project.display()
        ),
    );

    let first = discover_inventory(home.clone(), vec![project.clone()]);
    let revision = |snapshot: &super::models::InventorySnapshot,
                    client: ClientKind,
                    item_type: InventoryItemType,
                    name: &str| {
        snapshot
            .records
            .iter()
            .find(|record| {
                record.client == client && record.item_type == item_type && record.name == name
            })
            .expect("stateful fixture should be discovered")
            .action_capabilities
            .source_revision
            .clone()
            .expect("discovered record should have a revision")
    };
    let first_skill = revision(
        &first,
        ClientKind::Codex,
        InventoryItemType::Skill,
        "shared-skill",
    );
    let first_hook = revision(
        &first,
        ClientKind::Codex,
        InventoryItemType::Hook,
        "Stop hook",
    );
    let first_project_mcp = revision(
        &first,
        ClientKind::Claude,
        InventoryItemType::Mcp,
        "stateful-project",
    );

    write(
        &home.join(".codex/config.toml"),
        &format!(
            "[features]\ncodex_hooks = false\n\n[[skills.config]]\npath = '{}'\nenabled = false\n",
            skill_path.display()
        ),
    );
    write(
        &home.join(".claude.json"),
        &format!(
            r#"{{"projects":{{"{}":{{"hasTrustDialogAccepted":true,"enableAllProjectMcpServers":true,"disabledMcpjsonServers":["stateful-project"]}}}}}}"#,
            project.display()
        ),
    );

    let changed = discover_inventory(home, vec![project]);
    assert_ne!(
        first_skill,
        revision(
            &changed,
            ClientKind::Codex,
            InventoryItemType::Skill,
            "shared-skill"
        )
    );
    assert_ne!(
        first_hook,
        revision(
            &changed,
            ClientKind::Codex,
            InventoryItemType::Hook,
            "Stop hook"
        )
    );
    assert_ne!(
        first_project_mcp,
        revision(
            &changed,
            ClientKind::Claude,
            InventoryItemType::Mcp,
            "stateful-project"
        )
    );
}

#[test]
fn claude_action_revisions_cover_managed_policy_sources() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let managed_settings = fixture.path().join("managed-settings.json");
    let managed_mcp = fixture.path().join("managed-mcp.json");
    write(
        &home.join(".claude.json"),
        r#"{"mcpServers":{"policy-sensitive":{"command":"server"}}}"#,
    );
    let revision = |snapshot: &InventorySnapshot| {
        snapshot
            .records
            .iter()
            .find(|record| record.client == ClientKind::Claude && record.name == "policy-sensitive")
            .expect("Claude MCP should be discovered")
            .action_capabilities
            .source_revision
            .clone()
    };

    let initial = discover_inventory_with_paths(
        home.clone(),
        home.join(".codex"),
        managed_settings.clone(),
        managed_mcp.clone(),
        Vec::new(),
    );
    let initial_revision = revision(&initial);

    write(
        &managed_settings,
        r#"{"deniedMcpServers":["policy-sensitive"]}"#,
    );
    let policy_changed = discover_inventory_with_paths(
        home.clone(),
        home.join(".codex"),
        managed_settings.clone(),
        managed_mcp.clone(),
        Vec::new(),
    );
    let policy_revision = revision(&policy_changed);
    assert_ne!(initial_revision, policy_revision);

    write(
        &managed_mcp,
        r#"{"mcpServers":{"managed":{"command":"managed-server"}}}"#,
    );
    let managed_mcp_created = discover_inventory_with_paths(
        home.clone(),
        home.join(".codex"),
        managed_settings,
        managed_mcp,
        Vec::new(),
    );
    assert_ne!(policy_revision, revision(&managed_mcp_created));
}

#[test]
fn only_warnings_with_explicit_blocked_reasons_restrict_sources() {
    let ordinary = InventoryWarning::new(
        ClientKind::Codex,
        "/fixture/unreadable-skill",
        "Skipped an unreadable or cyclic skill path.",
    );
    assert_eq!(warning_source_restriction(&ordinary), None);

    let malformed = InventoryWarning::blocked(
        ClientKind::Codex,
        "/fixture/config.toml",
        "Skipped a malformed entry.",
        ActionBlockedReason::MalformedSource,
    );
    assert_eq!(
        warning_source_restriction(&malformed),
        Some((
            "/fixture/config.toml".to_string(),
            ActionBlockedReason::MalformedSource
        ))
    );
}

#[test]
fn first_source_restriction_preserves_its_specific_reason() {
    let source_path = "/fixture/broken-link.json";
    let mut snapshot = InventorySnapshot::new(0);
    snapshot.restrict_source(source_path, ActionBlockedReason::BrokenSymlink);

    let (_, restriction) = snapshot.composite_source_revision(&[source_path.to_string()]);

    assert_eq!(restriction, Some(ActionBlockedReason::BrokenSymlink));
}

#[test]
fn claude_project_trust_applies_to_project_skills() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let project = fixture.path().join("project");
    write(
        &home.join(".claude.json"),
        &format!(
            r#"{{"projects":{{"{}":{{"hasTrustDialogAccepted":false}}}}}}"#,
            project.display()
        ),
    );
    write_skill(&project.join(".claude/skills/untrusted-skill/SKILL.md"));

    let snapshot = discover_inventory(home, vec![project]);
    let skill = snapshot
        .records
        .iter()
        .find(|record| record.client == ClientKind::Claude && record.name == "shared-skill")
        .expect("Claude project skill should be discovered");

    assert_eq!(skill.trust_state, TrustState::Untrusted);
    assert_eq!(skill.is_effective, Some(false));
}

#[test]
fn malformed_hook_handlers_are_skipped() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    write(
        &home.join(".cursor/hooks.json"),
        r#"{
            "hooks": {
                "Stop": [
                    {},
                    {"type":"command"},
                    {"type":"command","command":"valid-command"}
                ]
            }
        }"#,
    );

    let snapshot = discover_inventory(home, Vec::new());
    let hooks: Vec<_> = snapshot
        .records
        .iter()
        .filter(|record| {
            record.client == ClientKind::Cursor && record.item_type == InventoryItemType::Hook
        })
        .collect();

    assert_eq!(hooks.len(), 1);
    assert!(snapshot.warnings.iter().any(|warning| {
        warning.client == Some(ClientKind::Cursor) && warning.message.contains("no usable payload")
    }));
}

#[test]
fn unclosed_skill_frontmatter_is_not_effective() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    write(
        &home.join(".agents/skills/folder-name/SKILL.md"),
        "---\nname: misleading-name\ndescription: missing closing delimiter\n",
    );

    let snapshot = discover_inventory(home, Vec::new());
    let skill = snapshot
        .records
        .iter()
        .find(|record| record.client == ClientKind::Codex && record.name == "folder-name")
        .expect("malformed skill should remain visible by its folder name");

    assert_eq!(skill.is_effective, Some(false));
    assert!(snapshot
        .warnings
        .iter()
        .any(|warning| warning.message.contains("frontmatter name")));
}

#[test]
fn codex_hook_trust_requires_the_current_handler_hash() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let config_path = home.join(".codex/config.toml");
    let hooks_path = home.join(".codex/hooks.json");
    write(
        &hooks_path,
        r#"{
            "hooks": {
                "PreToolUse": [
                    {"hooks":[{"type":"command","command":"echo trusted"}]},
                    {"hooks":[{"type":"command","command":"echo changed"}]}
                ]
            }
        }"#,
    );
    write(
        &config_path,
        &format!(
            r#"[hooks.state."{}:pre_tool_use:0:0"]
trusted_hash = "sha256:620fb822b32c78c73a2c5817199b662d4e82c221d93dd1b85bf843cf8fec7785"

[hooks.state."{}:pre_tool_use:1:0"]
trusted_hash = "sha256:620fb822b32c78c73a2c5817199b662d4e82c221d93dd1b85bf843cf8fec7785"
"#,
            hooks_path.display(),
            hooks_path.display()
        ),
    );

    let snapshot = discover_inventory(home, Vec::new());
    let hooks: Vec<_> = snapshot
        .records
        .iter()
        .filter(|record| {
            record.client == ClientKind::Codex && record.item_type == InventoryItemType::Hook
        })
        .collect();

    assert_eq!(hooks.len(), 2);
    assert_eq!(hooks[0].trust_state, TrustState::Trusted);
    assert_eq!(hooks[0].is_effective, Some(true));
    assert_eq!(hooks[1].trust_state, TrustState::Untrusted);
    assert_eq!(hooks[1].is_effective, Some(false));
    assert!(hooks[1].action_capabilities.enable.available);
    assert!(hooks[1].action_capabilities.disable.available);
}

#[test]
fn discovers_components_from_configured_codex_plugins() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let marketplace = fixture.path().join("marketplace");
    let plugin = marketplace.join("plugins/demo");
    write(
        &marketplace.join(".agents/plugins/marketplace.json"),
        r#"{"name":"local","plugins":[{"name":"demo","source":"./plugins/demo"}]}"#,
    );
    write(
        &plugin.join(".codex-plugin/plugin.json"),
        r#"{
            "name":"demo",
            "skills":"./skills/",
            "mcpServers":"./.mcp.json",
            "hooks":"./hooks/hooks.json"
        }"#,
    );
    write_skill(&plugin.join("skills/plugin-skill/SKILL.md"));
    write(
        &plugin.join(".mcp.json"),
        r#"{"mcpServers":{"plugin-server":{"command":"plugin-command"}}}"#,
    );
    write(
        &plugin.join("hooks/hooks.json"),
        r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"plugin-hook"}]}]}}"#,
    );
    write(
        &home.join(".codex/config.toml"),
        &format!(
            r#"[marketplaces.local]
source_type = "local"
source = "{}"

[plugins."demo@local"]
enabled = true
"#,
            marketplace.display()
        ),
    );

    let snapshot = discover_inventory(home, Vec::new());

    for (name, item_type) in [
        ("shared-skill", InventoryItemType::Skill),
        ("plugin-server", InventoryItemType::Mcp),
        ("Stop hook", InventoryItemType::Hook),
    ] {
        assert!(snapshot.records.iter().any(|record| {
            record.client == ClientKind::Codex
                && record.name == name
                && record.item_type == item_type
        }));
    }
}

#[test]
fn preserves_same_named_claude_servers_from_multiple_projects() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    write(
        &home.join(".claude.json"),
        r#"{
            "mcpServers": {"shared": {"command": "user-server"}},
            "projects": {
                "/projects/one": {"mcpServers": {"shared": {"command": "one-server"}}},
                "/projects/two": {"mcpServers": {"shared": {"command": "two-server"}}}
            }
        }"#,
    );

    let snapshot = discover_inventory(home, Vec::new());
    let records: Vec<_> = snapshot
        .records
        .iter()
        .filter(|record| record.client == ClientKind::Claude && record.name == "shared")
        .collect();
    let unique_ids: std::collections::HashSet<_> =
        records.iter().map(|record| &record.id).collect();

    assert_eq!(records.len(), 3);
    assert_eq!(unique_ids.len(), 3);
}

#[test]
fn applies_claude_project_disable_and_trust_state() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let project = fixture.path().join("project");
    write(
        &home.join(".claude.json"),
        &format!(
            r#"{{
                "projects": {{
                    "{}": {{
                        "disabledMcpjsonServers": ["shared"],
                        "hasTrustDialogAccepted": false
                    }}
                }}
            }}"#,
            project.display()
        ),
    );
    write(
        &project.join(".mcp.json"),
        r#"{"mcpServers": {"shared": {"command": "project-server"}}}"#,
    );

    let snapshot = discover_inventory(home, vec![project]);
    let record = snapshot
        .records
        .iter()
        .find(|record| {
            record.client == ClientKind::Claude
                && record.name == "shared"
                && record.source_kind == SourceKind::ProjectConfig
        })
        .expect("project MCP should be discovered");

    assert_eq!(record.enabled, Some(false));
    assert_eq!(record.is_effective, Some(false));
    assert!(serde_json::to_string(record)
        .unwrap()
        .contains(r#""trustState":"untrusted""#));
}

#[test]
fn uses_codex_home_environment_override() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let codex_home = fixture.path().join("custom-codex-home");
    write(
        &codex_home.join("config.toml"),
        "[mcp_servers.custom-home]\ncommand = 'custom-server'\n",
    );
    let resolved_codex_home = codex_home_path(&home, Some(codex_home.clone().into_os_string()));
    let snapshot = discover_inventory_with_codex_home(home, resolved_codex_home, Vec::new());

    assert!(snapshot.records.iter().any(|record| {
        record.client == ClientKind::Codex
            && record.name == "custom-home"
            && record.source_path == codex_home.join("config.toml").display().to_string()
    }));
}

#[test]
fn reports_context_dependent_codex_mcp_precedence() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let project = fixture.path().join("project");
    write(
        &home.join(".codex/config.toml"),
        "[mcp_servers.shared]\ncommand = 'user-server'\n",
    );
    write(
        &project.join(".codex/config.toml"),
        "[mcp_servers.shared]\ncommand = 'project-server'\n",
    );

    let snapshot = discover_inventory(home, vec![project]);
    let records: Vec<_> = snapshot
        .records
        .iter()
        .filter(|record| record.client == ClientKind::Codex && record.name == "shared")
        .collect();

    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|record| record.is_effective.is_none()));
    assert!(records.iter().any(|record| record.source_priority == 100));
    assert!(records.iter().any(|record| record.source_priority == 200));
}

#[test]
fn warns_when_a_registered_project_cannot_be_scanned() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let missing_project = fixture.path().join("missing-project");

    let snapshot = discover_inventory(home, vec![missing_project.clone()]);

    assert_eq!(snapshot.scanned_project_count, 0);
    assert!(snapshot.warnings.iter().any(|warning| {
        warning.source_path == missing_project.display().to_string()
            && warning.message.contains("registered project")
    }));
}

#[test]
fn pending_claude_project_mcp_is_not_effective() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let project = fixture.path().join("project");
    let allow_all_project = fixture.path().join("allow-all-project");
    write(
        &home.join(".claude.json"),
        &format!(
            r#"{{
                "projects": {{
                    "{}": {{
                        "enabledMcpjsonServers": ["approved"],
                        "disabledMcpjsonServers": [],
                        "hasTrustDialogAccepted": true
                    }},
                    "{}": {{
                        "enableAllProjectMcpServers": true,
                        "hasTrustDialogAccepted": true
                    }}
                }}
            }}"#,
            project.display(),
            allow_all_project.display()
        ),
    );
    write(
        &project.join(".mcp.json"),
        r#"{
            "mcpServers": {
                "approved": {"command": "approved-server"},
                "pending": {"command": "pending-server"}
            }
        }"#,
    );
    write(
        &allow_all_project.join(".mcp.json"),
        r#"{"mcpServers":{"automatically-approved":{"command":"approved-server"}}}"#,
    );

    let snapshot = discover_inventory(home, vec![project, allow_all_project]);
    let approved = snapshot
        .records
        .iter()
        .find(|record| record.client == ClientKind::Claude && record.name == "approved")
        .expect("approved project MCP should be discovered");
    let pending = snapshot
        .records
        .iter()
        .find(|record| record.client == ClientKind::Claude && record.name == "pending")
        .expect("pending project MCP should be discovered");
    let automatically_approved = snapshot
        .records
        .iter()
        .find(|record| record.name == "automatically-approved")
        .expect("project MCP should be discovered when all servers are approved");

    assert_eq!(approved.enabled, Some(true));
    assert_eq!(approved.is_effective, Some(true));
    assert_eq!(pending.enabled, Some(true));
    assert_eq!(pending.is_effective, Some(false));
    assert!(pending.action_capabilities.enable.available);
    assert_eq!(pending.action_capabilities.enable.blocked_reason, None);
    assert!(pending.action_capabilities.disable.available);
    assert_eq!(automatically_approved.is_effective, Some(true));
}

#[test]
fn claude_hook_disable_uses_highest_precedence_setting() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let inherited_project = fixture.path().join("inherited-project");
    let local_project = fixture.path().join("local-project");
    let enabled_project = fixture.path().join("enabled-project");
    write(
        &home.join(".claude/settings.json"),
        r#"{"disableAllHooks": true}"#,
    );
    write(
        &home.join(".claude.json"),
        &format!(
            r#"{{"projects":{{"{}":{{"hasTrustDialogAccepted":true}},"{}":{{"hasTrustDialogAccepted":true}},"{}":{{"hasTrustDialogAccepted":true}}}}}}"#,
            inherited_project.display(),
            local_project.display(),
            enabled_project.display()
        ),
    );
    for project in [&inherited_project, &local_project, &enabled_project] {
        write(
            &project.join(".claude/settings.json"),
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"shared-hook"}]}]}}"#,
        );
    }
    write(
        &local_project.join(".claude/settings.json"),
        r#"{"disableAllHooks":false,"hooks":{"Stop":[{"hooks":[{"type":"command","command":"shared-hook"}]}]}}"#,
    );
    write(
        &local_project.join(".claude/settings.local.json"),
        r#"{"disableAllHooks":true}"#,
    );
    write(
        &enabled_project.join(".claude/settings.json"),
        r#"{"disableAllHooks":false,"hooks":{"Stop":[{"hooks":[{"type":"command","command":"shared-hook"}]}]}}"#,
    );

    let snapshot = discover_inventory(
        home,
        vec![
            inherited_project.clone(),
            local_project.clone(),
            enabled_project.clone(),
        ],
    );
    let project_hook = |project: &Path| {
        let project_path = fs::canonicalize(project).unwrap().display().to_string();
        snapshot
            .records
            .iter()
            .find(|record| {
                record.client == ClientKind::Claude
                    && record.item_type == InventoryItemType::Hook
                    && record.project_path.as_deref() == Some(project_path.as_str())
            })
            .expect("project hook should be discovered")
    };

    assert_eq!(project_hook(&inherited_project).is_effective, Some(false));
    assert_eq!(project_hook(&local_project).is_effective, Some(false));
    assert_eq!(project_hook(&enabled_project).is_effective, Some(true));
}

#[test]
fn codex_project_trust_controls_effectiveness() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let trusted_project = fixture.path().join("trusted-project");
    let untrusted_project = fixture.path().join("untrusted-project");
    let canonical_fixture = fs::canonicalize(fixture.path()).unwrap();
    write(
        &home.join(".codex/config.toml"),
        &format!(
            "[projects.'{}']\ntrust_level = 'trusted'\n\n[projects.'{}']\ntrust_level = 'untrusted'\n\n[hooks.state.'{}:stop:0:0']\ntrusted_hash = 'sha256:0ce4021f58cfb1de2385dbe69ddcb635e9f4da01a9c8e32964b212c20cf81126'\n\n[hooks.state.'{}:stop:0:0']\ntrusted_hash = 'sha256:8226a59c81798974e69fa3bfc6fd11bfed8073bb1399d6588b6728d88765e0e9'\n",
            trusted_project.display(),
            untrusted_project.display(),
            canonical_fixture
                .join("trusted-project/.codex/config.toml")
                .display(),
            canonical_fixture
                .join("untrusted-project/.codex/config.toml")
                .display()
        ),
    );
    write(
        &trusted_project.join(".codex/config.toml"),
        "[mcp_servers.trusted-server]\ncommand = 'trusted'\n\n[[hooks.Stop]]\n\n[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = 'trusted-hook'\n",
    );
    write(
        &untrusted_project.join(".codex/config.toml"),
        "[mcp_servers.untrusted-server]\ncommand = 'untrusted'\n\n[[hooks.Stop]]\n\n[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = 'untrusted-hook'\n",
    );

    let snapshot = discover_inventory(home, vec![trusted_project, untrusted_project]);
    let trusted = snapshot
        .records
        .iter()
        .find(|record| record.name == "trusted-server")
        .expect("trusted project MCP should be discovered");
    let untrusted = snapshot
        .records
        .iter()
        .find(|record| record.name == "untrusted-server")
        .expect("untrusted project MCP should be discovered");

    assert_eq!(trusted.trust_state, TrustState::Trusted);
    assert_eq!(trusted.is_effective, Some(true));
    assert_eq!(untrusted.trust_state, TrustState::Untrusted);
    assert_eq!(untrusted.is_effective, Some(false));
    let project_hooks: Vec<_> = snapshot
        .records
        .iter()
        .filter(|record| {
            record.client == ClientKind::Codex && record.item_type == InventoryItemType::Hook
        })
        .collect();
    assert!(project_hooks.iter().any(|record| {
        record.trust_state == TrustState::Trusted && record.is_effective == Some(true)
    }));
    assert!(project_hooks.iter().any(|record| {
        record.trust_state == TrustState::Untrusted && record.is_effective == Some(false)
    }));
}

#[test]
fn codex_inline_hooks_use_effective_feature_state() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let project = fixture.path().join("project");
    write(
        &home.join(".codex/config.toml"),
        "[features]\ncodex_hooks = false\n\n[[hooks.Stop]]\n\n[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = 'user-hook'\n",
    );
    write(
        &project.join(".codex/config.toml"),
        "[[hooks.Stop]]\n\n[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = 'project-hook'\n",
    );

    let snapshot = discover_inventory(home, vec![project]);
    let hooks: Vec<_> = snapshot
        .records
        .iter()
        .filter(|record| {
            record.client == ClientKind::Codex && record.item_type == InventoryItemType::Hook
        })
        .collect();

    assert_eq!(hooks.len(), 2);
    assert!(hooks.iter().all(|record| record.enabled == Some(false)));
    assert!(hooks
        .iter()
        .all(|record| record.is_effective == Some(false)));
}

#[test]
fn duplicate_codex_skills_report_contextual_precedence() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let project = fixture.path().join("project");
    write_skill(&home.join(".agents/skills/shared-skill/SKILL.md"));
    write_skill(&project.join(".agents/skills/shared-skill/SKILL.md"));
    write_skill(&project.join(".codex/skills/shared-skill/SKILL.md"));
    write(
        &home.join(".codex/config.toml"),
        &format!(
            "[projects.'{}']\ntrust_level = 'trusted'\n",
            project.display()
        ),
    );

    let snapshot = discover_inventory(home, vec![project]);
    let skills: Vec<_> = snapshot
        .records
        .iter()
        .filter(|record| {
            record.client == ClientKind::Codex
                && record.item_type == InventoryItemType::Skill
                && record.name == "shared-skill"
        })
        .collect();

    assert_eq!(skills.len(), 3);
    let user = skills
        .iter()
        .find(|record| record.scope == super::models::InventoryScope::User)
        .expect("user skill should be discovered");
    let project_skill = skills
        .iter()
        .find(|record| record.source_kind == SourceKind::ProjectSkills)
        .expect("project skill should be discovered");
    let legacy = skills
        .iter()
        .find(|record| record.source_kind == SourceKind::LegacySkills)
        .expect("legacy project skill should be discovered");

    assert_eq!(user.is_effective, None);
    assert_eq!(project_skill.is_effective, Some(true));
    assert_eq!(legacy.is_effective, Some(false));
}

#[test]
fn claude_managed_policy_controls_hooks_and_mcps() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let managed_settings = fixture.path().join("managed-settings.json");
    let managed_mcp = fixture.path().join("managed-mcp-missing.json");
    write(
        &home.join(".claude.json"),
        r#"{
            "mcpServers": {
                "allowed": {"command": "allowed-server"},
                "denied": {"command": "denied-server"},
                "unlisted": {"command": "unlisted-server"}
            }
        }"#,
    );
    write(
        &home.join(".claude/settings.json"),
        r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"user-hook"}]}]}}"#,
    );
    write(
        &managed_settings,
        r#"{
            "allowManagedHooksOnly": true,
            "allowedMcpServers": [{"serverName": "allowed"}],
            "deniedMcpServers": [{"serverName": "denied"}],
            "hooks":{"Stop":[{"hooks":[{"type":"command","command":"managed-hook"}]}]}
        }"#,
    );

    let snapshot = discover_inventory_with_paths(
        home.clone(),
        home.join(".codex"),
        managed_settings,
        managed_mcp,
        Vec::new(),
    );
    let mcp = |name: &str| {
        snapshot
            .records
            .iter()
            .find(|record| record.client == ClientKind::Claude && record.name == name)
            .expect("Claude MCP should be discovered")
    };
    assert_eq!(mcp("allowed").is_effective, Some(true));
    assert_eq!(mcp("denied").is_effective, Some(false));
    assert_eq!(mcp("unlisted").is_effective, Some(false));

    let hooks: Vec<_> = snapshot
        .records
        .iter()
        .filter(|record| {
            record.client == ClientKind::Claude && record.item_type == InventoryItemType::Hook
        })
        .collect();
    assert!(hooks.iter().any(|record| {
        record.source_kind == SourceKind::ManagedConfig && record.is_effective == Some(true)
    }));
    assert!(hooks.iter().any(|record| {
        record.source_kind == SourceKind::UserConfig && record.is_effective == Some(false)
    }));
}

#[test]
fn managed_mcp_file_excludes_user_configured_servers() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let managed_settings = fixture.path().join("managed-settings-missing.json");
    let managed_mcp = fixture.path().join("managed-mcp.json");
    write(
        &home.join(".claude.json"),
        r#"{"mcpServers":{"shared-server":{"command":"user-command"}}}"#,
    );
    write(
        &managed_mcp,
        r#"{"mcpServers":{"shared-server":{"command":"managed-command"}}}"#,
    );

    let snapshot = discover_inventory_with_paths(
        home.clone(),
        home.join(".codex"),
        managed_settings,
        managed_mcp,
        Vec::new(),
    );
    let user = snapshot
        .records
        .iter()
        .find(|record| record.source_kind == SourceKind::UserConfig)
        .expect("user MCP should remain visible");
    let managed = snapshot
        .records
        .iter()
        .find(|record| record.source_kind == SourceKind::ManagedConfig)
        .expect("managed MCP should be discovered");

    assert_eq!(user.is_effective, Some(false));
    assert_eq!(managed.scope, super::models::InventoryScope::Admin);
    assert_eq!(managed.source_kind, SourceKind::ManagedConfig);
    assert_eq!(managed.is_effective, Some(true));
}

#[test]
fn disabled_project_mcp_still_shadows_user_definition() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let project = fixture.path().join("project");
    write(
        &home.join(".codex/config.toml"),
        &format!(
            "[mcp_servers.shared]\ncommand = 'user'\n\n[projects.\"{}\"]\ntrust_level = 'trusted'\n",
            project.display()
        ),
    );
    write(
        &project.join(".codex/config.toml"),
        "[mcp_servers.shared]\ncommand = 'project'\nenabled = false\n",
    );

    let snapshot = discover_inventory(home, vec![project]);
    let records: Vec<_> = snapshot
        .records
        .iter()
        .filter(|record| record.client == ClientKind::Codex && record.name == "shared")
        .collect();
    let user = records
        .iter()
        .find(|record| record.scope == super::models::InventoryScope::User)
        .expect("user MCP should be discovered");
    let project = records
        .iter()
        .find(|record| record.scope == super::models::InventoryScope::Project)
        .expect("project MCP should be discovered");

    assert_eq!(user.is_effective, None);
    assert_eq!(project.enabled, Some(false));
    assert_eq!(project.is_effective, Some(false));
}

#[test]
fn managed_disable_all_hooks_applies_to_every_scope() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let managed_settings = fixture.path().join("managed-settings.json");
    let managed_mcp = fixture.path().join("managed-mcp-missing.json");
    write(
        &home.join(".claude/settings.json"),
        r#"{"disableAllHooks":false,"hooks":{"Stop":[{"hooks":[{"type":"command","command":"user-hook"}]}]}}"#,
    );
    write(
        &managed_settings,
        r#"{"disableAllHooks":true,"hooks":{"Stop":[{"hooks":[{"type":"command","command":"managed-hook"}]}]}}"#,
    );

    let snapshot = discover_inventory_with_paths(
        home.clone(),
        home.join(".codex"),
        managed_settings,
        managed_mcp,
        Vec::new(),
    );
    let hooks: Vec<_> = snapshot
        .records
        .iter()
        .filter(|record| {
            record.client == ClientKind::Claude && record.item_type == InventoryItemType::Hook
        })
        .collect();

    assert_eq!(hooks.len(), 2);
    assert!(hooks
        .iter()
        .all(|record| record.enabled == Some(false) && record.is_effective == Some(false)));
}

#[test]
fn discovers_valid_top_level_skill_with_pruned_directory_name() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    write(
        &home.join(".agents/skills/build/SKILL.md"),
        "---\nname: build\ndescription: Build workflow\n---\n",
    );

    let snapshot = discover_inventory(home, Vec::new());

    assert!(snapshot.records.iter().any(|record| {
        record.client == ClientKind::Codex
            && record.item_type == InventoryItemType::Skill
            && record.name == "build"
    }));
}

#[test]
fn ignores_skill_files_nested_inside_a_skill() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    write(
        &home.join(".agents/skills/parent/SKILL.md"),
        "---\nname: parent\ndescription: Parent skill\n---\n",
    );
    write(
        &home.join(".agents/skills/parent/references/example/SKILL.md"),
        "---\nname: nested-support-file\ndescription: Support fixture\n---\n",
    );

    let snapshot = discover_inventory(home, Vec::new());

    assert!(snapshot.records.iter().any(|record| {
        record.client == ClientKind::Codex
            && record.item_type == InventoryItemType::Skill
            && record.name == "parent"
    }));
    assert!(!snapshot.records.iter().any(|record| {
        record.client == ClientKind::Codex
            && record.item_type == InventoryItemType::Skill
            && record.name == "nested-support-file"
    }));
}

#[test]
fn required_skill_name_missing_is_not_effective() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    write(
        &home.join(".agents/skills/missing-name/SKILL.md"),
        "---\ndescription: Missing name fixture\n---\n",
    );

    let snapshot = discover_inventory(home, Vec::new());
    let skill = snapshot
        .records
        .iter()
        .find(|record| record.client == ClientKind::Codex && record.name == "missing-name")
        .expect("invalid skill should remain visible");

    assert_eq!(skill.enabled, Some(true));
    assert_eq!(skill.is_effective, Some(false));
    assert!(snapshot.warnings.iter().any(|warning| {
        warning.source_path.ends_with("missing-name/SKILL.md")
            && warning.message.contains("frontmatter name")
    }));
}

#[test]
fn malformed_mcp_transports_are_skipped_with_warnings() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    write(
        &home.join(".claude.json"),
        r#"{"mcpServers":{"valid-json":{"command":"server"},"empty-json":{},"null-url":{"url":null}}}"#,
    );
    write(
        &home.join(".codex/config.toml"),
        "[mcp_servers.valid-toml]\ncommand = 'server'\n\n[mcp_servers.empty-toml]\nenabled = true\n\n[mcp_servers.blank-url]\nurl = '   '\n",
    );

    let snapshot = discover_inventory(home, Vec::new());

    for name in ["valid-json", "valid-toml"] {
        let record = snapshot
            .records
            .iter()
            .find(|record| record.name == name)
            .expect("valid sibling should remain visible");
        assert_eq!(
            record.action_capabilities.disable.blocked_reason,
            Some(ActionBlockedReason::MalformedSource)
        );
    }
    for name in ["empty-json", "null-url", "empty-toml", "blank-url"] {
        assert!(!snapshot.records.iter().any(|record| record.name == name));
    }
    assert_eq!(
        snapshot
            .warnings
            .iter()
            .filter(|warning| warning.message.contains("usable transport"))
            .count(),
        4
    );
}

#[cfg(unix)]
#[test]
fn dangling_configuration_symlink_produces_a_warning() {
    use std::os::unix::fs::symlink;

    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let config_path = home.join(".cursor/mcp.json");
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    symlink(fixture.path().join("missing-mcp.json"), &config_path).unwrap();

    let snapshot = discover_inventory(home, Vec::new());

    assert!(snapshot.warnings.iter().any(|warning| {
        warning.source_path == config_path.display().to_string()
            && warning.message.contains("symlink")
            && warning.blocked_reason == Some(ActionBlockedReason::BrokenSymlink)
    }));
}

#[cfg(unix)]
#[test]
fn canonicalizes_local_claude_project_paths_before_precedence() {
    use std::os::unix::fs::symlink;

    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let project = fixture.path().join("project");
    let project_link = fixture.path().join("project-link");
    write(
        &project.join(".mcp.json"),
        r#"{"mcpServers":{"shared":{"command":"project-server"}}}"#,
    );
    symlink(&project, &project_link).unwrap();
    write(
        &home.join(".claude.json"),
        &format!(
            r#"{{"projects":{{"{}":{{"hasTrustDialogAccepted":true,"enableAllProjectMcpServers":true,"mcpServers":{{"shared":{{"command":"local-server"}}}}}}}}}}"#,
            project_link.display()
        ),
    );

    let snapshot = discover_inventory(home, vec![project_link]);
    let local = snapshot
        .records
        .iter()
        .find(|record| {
            record.client == ClientKind::Claude
                && record.name == "shared"
                && record.source_kind == SourceKind::LocalConfig
        })
        .expect("local MCP should be discovered");
    let project_config = snapshot
        .records
        .iter()
        .find(|record| {
            record.client == ClientKind::Claude
                && record.name == "shared"
                && record.source_kind == SourceKind::ProjectConfig
        })
        .expect("project MCP should be discovered");

    assert_eq!(local.project_path, project_config.project_path);
    assert_eq!(local.is_effective, Some(true));
    assert_eq!(project_config.is_effective, Some(false));
}

#[test]
fn malformed_hook_handlers_are_skipped_with_a_warning() {
    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    write(
        &home.join(".claude/settings.json"),
        r#"{"hooks":{"Stop":[{"hooks":[null,"bad",{"type":"command","command":"valid"}]}]}}"#,
    );

    let snapshot = discover_inventory(home, Vec::new());
    let hooks: Vec<_> = snapshot
        .records
        .iter()
        .filter(|record| {
            record.client == ClientKind::Claude && record.item_type == InventoryItemType::Hook
        })
        .collect();

    assert_eq!(hooks.len(), 1);
    assert!(snapshot
        .warnings
        .iter()
        .any(|warning| warning.message.contains("hook handler")));
}

#[cfg(unix)]
#[test]
fn symlinked_skills_resolve_and_cycles_become_warnings() {
    use std::os::unix::fs::symlink;

    let fixture = TempDir::new().unwrap();
    let home = fixture.path().join("home");
    let skills_root = home.join(".agents/skills");
    let target = fixture.path().join("shared/linked-skill");
    write(
        &target.join("SKILL.md"),
        "---\nname: linked-skill\ndescription: Linked fixture\n---\n",
    );
    fs::create_dir_all(&skills_root).unwrap();
    symlink(&target, skills_root.join("linked-skill")).unwrap();
    symlink(
        fixture.path().join("missing-skill"),
        skills_root.join("broken-skill"),
    )
    .unwrap();
    symlink(&skills_root, skills_root.join("cycle")).unwrap();

    let snapshot = discover_inventory(home, Vec::new());
    let linked = snapshot
        .records
        .iter()
        .find(|record| record.client == ClientKind::Codex && record.name == "linked-skill")
        .expect("symlinked skill should be discovered");
    assert!(linked.is_symlink);
    assert_ne!(linked.original_path, linked.resolved_path.clone().unwrap());
    assert!(snapshot.warnings.iter().any(|warning| {
        warning.source_path.ends_with("broken-skill")
            && warning.message.contains("broken")
            && warning.blocked_reason == Some(ActionBlockedReason::BrokenSymlink)
    }));
    assert!(snapshot.warnings.iter().any(|warning| {
        warning.source_path.ends_with("cycle") && warning.message.contains("cyclic")
    }));
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn write_skill(path: &Path) {
    write(path, SKILL);
}
