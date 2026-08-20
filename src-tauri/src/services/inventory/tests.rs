use super::discover_inventory;
use super::models::{ClientKind, InventoryItemType};
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
        warning.source_path.ends_with("broken-skill") && warning.message.contains("broken")
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
