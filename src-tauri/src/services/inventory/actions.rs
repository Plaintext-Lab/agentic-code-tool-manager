use super::atomic_config::{
    restore_original, AtomicConfigWriter, ConfigSource, ConfigWriteError, ConfigWriter,
};
use super::codex_mcp_config::update_codex_mcp_config;
use super::codex_skill_config::update_codex_skill_config;
use super::models::{ClientKind, InventoryItemType, InventoryRecord};
use super::{discover_inventory_with_codex_home, InventorySnapshot};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Identifies one discovered record and the exact scanned state the user confirmed.
pub struct InventoryActionRequest {
    pub record_id: String,
    pub enabled: bool,
    pub source_revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum InventoryActionError {
    #[error("This inventory item is no longer available. Scan again and retry.")]
    MissingRecord,
    #[error("This inventory item is ambiguous. Scan again before retrying.")]
    AmbiguousRecord,
    #[error("The inventory changed after it was scanned. Scan again and retry.")]
    StaleInventory,
    #[error("This inventory item does not support this action.")]
    UnsupportedRecord,
    #[error("This action is no longer available. Scan again and retry.")]
    ActionUnavailable,
    #[error("The Codex configuration is malformed. Fix it, then scan again.")]
    MalformedConfiguration,
    #[error("The Codex configuration path is not safe to replace.")]
    UnsafeConfiguration,
    #[error("The Codex configuration could not be updated. The original was kept.")]
    WriteFailed,
    #[error("Codex did not report the requested state after the update.")]
    VerificationFailed,
    #[error("The Codex state could not be verified or restored. Inspect the configuration, then scan again.")]
    RollbackFailed,
}

pub(super) fn set_inventory_record_enabled_with_paths(
    home_dir: PathBuf,
    codex_home: PathBuf,
    project_roots: Vec<PathBuf>,
    request: InventoryActionRequest,
) -> Result<InventorySnapshot, InventoryActionError> {
    set_inventory_record_enabled_with_writer(
        home_dir,
        codex_home,
        project_roots,
        request,
        &AtomicConfigWriter,
    )
}

fn set_inventory_record_enabled_with_writer(
    home_dir: PathBuf,
    codex_home: PathBuf,
    project_roots: Vec<PathBuf>,
    request: InventoryActionRequest,
    writer: &dyn ConfigWriter,
) -> Result<InventorySnapshot, InventoryActionError> {
    let initial = discover_inventory_with_codex_home(
        home_dir.clone(),
        codex_home.clone(),
        project_roots.clone(),
    );
    let record = resolve_record(&initial, &request.record_id)?;
    validate_request(record, &request)?;

    let config_path = match record.item_type {
        InventoryItemType::Skill => codex_home.join("config.toml"),
        InventoryItemType::Mcp => PathBuf::from(&record.source_path),
        InventoryItemType::Hook => return Err(InventoryActionError::UnsupportedRecord),
    };
    let source = ConfigSource::read(&config_path)?;
    let original = source.contents.as_deref().unwrap_or_default();
    let updated = match record.item_type {
        InventoryItemType::Skill => update_codex_skill_config(original, record, request.enabled)?,
        InventoryItemType::Mcp => update_codex_mcp_config(original, &request, &record.name)?,
        InventoryItemType::Hook => return Err(InventoryActionError::UnsupportedRecord),
    };
    let mut expected = initial.clone();
    expected.record_source_revision(&config_path, updated.as_bytes());
    let (expected_revision, expected_restriction) = expected.composite_source_revision(&[
        record.source_path.clone(),
        config_path.display().to_string(),
    ]);
    if expected_restriction.is_some() {
        return Err(InventoryActionError::StaleInventory);
    }

    let current = discover_inventory_with_codex_home(
        home_dir.clone(),
        codex_home.clone(),
        project_roots.clone(),
    );
    let current_record = resolve_record(&current, &request.record_id)?;
    validate_request(current_record, &request)?;

    writer
        .replace(&source, updated.as_bytes())
        .map_err(|error| match error {
            ConfigWriteError::SourceChanged => InventoryActionError::StaleInventory,
            ConfigWriteError::Io => InventoryActionError::WriteFailed,
            ConfigWriteError::RollbackFailed => InventoryActionError::RollbackFailed,
        })?;

    let verified = discover_inventory_with_codex_home(home_dir, codex_home, project_roots);
    let verified_record = resolve_record(&verified, &request.record_id).ok();
    if verified_record.and_then(|record| record.enabled) != Some(request.enabled) {
        restore_original(&source, updated.as_bytes())?;
        return Err(InventoryActionError::VerificationFailed);
    }
    if verified_record.and_then(|record| record.action_capabilities.source_revision.as_deref())
        != Some(expected_revision.as_str())
    {
        restore_original(&source, updated.as_bytes())?;
        return Err(InventoryActionError::StaleInventory);
    }
    Ok(verified)
}

fn resolve_record<'a>(
    snapshot: &'a InventorySnapshot,
    record_id: &str,
) -> Result<&'a InventoryRecord, InventoryActionError> {
    let mut matches = snapshot
        .records
        .iter()
        .filter(|record| record.id == record_id);
    let record = matches.next().ok_or(InventoryActionError::MissingRecord)?;
    if matches.next().is_some() {
        return Err(InventoryActionError::AmbiguousRecord);
    }
    Ok(record)
}

fn validate_request(
    record: &InventoryRecord,
    request: &InventoryActionRequest,
) -> Result<(), InventoryActionError> {
    if record.client != ClientKind::Codex
        || !matches!(
            record.item_type,
            InventoryItemType::Skill | InventoryItemType::Mcp
        )
    {
        return Err(InventoryActionError::UnsupportedRecord);
    }
    if !cfg!(target_os = "macos") {
        return Err(InventoryActionError::UnsupportedRecord);
    }
    if record.action_capabilities.source_revision.as_deref()
        != Some(request.source_revision.as_str())
    {
        return Err(InventoryActionError::StaleInventory);
    }
    let action = if request.enabled {
        &record.action_capabilities.enable
    } else {
        &record.action_capabilities.disable
    };
    if !action.available {
        return Err(InventoryActionError::ActionUnavailable);
    }
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    struct FailingWriter;

    impl ConfigWriter for FailingWriter {
        fn replace(&self, _source: &ConfigSource, _updated: &[u8]) -> Result<(), ConfigWriteError> {
            Err(ConfigWriteError::Io)
        }
    }

    struct SkillMutatingWriter {
        skill_file: PathBuf,
    }

    impl ConfigWriter for SkillMutatingWriter {
        fn replace(&self, source: &ConfigSource, updated: &[u8]) -> Result<(), ConfigWriteError> {
            AtomicConfigWriter.replace(source, updated)?;
            fs::write(
                &self.skill_file,
                "---\nname: rollback\ndescription: Changed during write\n---\n",
            )
            .map_err(|_| ConfigWriteError::Io)
        }
    }

    #[test]
    fn mcp_write_failure_keeps_the_original_config() {
        let fixture = TempDir::new().unwrap();
        let home = fixture.path().join("home");
        let codex_home = home.join(".codex");
        let config_path = codex_home.join("config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(
            &config_path,
            "[mcp_servers.rollback]\ncommand = 'TOP_SECRET_COMMAND'\nenabled = true\n",
        )
        .unwrap();
        let original = fs::read(&config_path).unwrap();
        let snapshot =
            discover_inventory_with_codex_home(home.clone(), codex_home.clone(), Vec::new());
        let record = snapshot
            .records
            .iter()
            .find(|record| {
                record.client == ClientKind::Codex
                    && record.item_type == InventoryItemType::Mcp
                    && record.name == "rollback"
            })
            .unwrap();

        let error = set_inventory_record_enabled_with_writer(
            home,
            codex_home,
            Vec::new(),
            InventoryActionRequest {
                record_id: record.id.clone(),
                enabled: false,
                source_revision: record.action_capabilities.source_revision.clone().unwrap(),
            },
            &FailingWriter,
        )
        .unwrap_err();

        assert_eq!(error, InventoryActionError::WriteFailed);
        assert_eq!(fs::read(config_path).unwrap(), original);
        assert!(!error.to_string().contains("TOP_SECRET_COMMAND"));
    }

    #[test]
    fn write_failure_keeps_the_original_config() {
        let fixture = TempDir::new().unwrap();
        let home = fixture.path().join("home");
        let codex_home = home.join(".codex");
        let skill_file = home.join(".agents/skills/rollback/SKILL.md");
        fs::create_dir_all(skill_file.parent().unwrap()).unwrap();
        fs::write(
            &skill_file,
            "---\nname: rollback\ndescription: Rollback fixture\n---\n",
        )
        .unwrap();
        let config_path = codex_home.join("config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, "# original remains\n").unwrap();
        let original = fs::read(&config_path).unwrap();
        let snapshot =
            discover_inventory_with_codex_home(home.clone(), codex_home.clone(), Vec::new());
        let record = snapshot
            .records
            .iter()
            .find(|record| record.client == ClientKind::Codex && record.name == "rollback")
            .unwrap();

        let error = set_inventory_record_enabled_with_writer(
            home,
            codex_home,
            Vec::new(),
            InventoryActionRequest {
                record_id: record.id.clone(),
                enabled: false,
                source_revision: record.action_capabilities.source_revision.clone().unwrap(),
            },
            &FailingWriter,
        )
        .unwrap_err();

        assert_eq!(error, InventoryActionError::WriteFailed);
        assert_eq!(fs::read(config_path).unwrap(), original);
    }

    #[test]
    fn skill_revision_change_during_write_restores_the_original_config() {
        let fixture = TempDir::new().unwrap();
        let home = fixture.path().join("home");
        let codex_home = home.join(".codex");
        let skill_file = home.join(".agents/skills/rollback/SKILL.md");
        fs::create_dir_all(skill_file.parent().unwrap()).unwrap();
        fs::write(
            &skill_file,
            "---\nname: rollback\ndescription: Rollback fixture\n---\n",
        )
        .unwrap();
        let config_path = codex_home.join("config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, "# original remains\n").unwrap();
        let original = fs::read(&config_path).unwrap();
        let snapshot =
            discover_inventory_with_codex_home(home.clone(), codex_home.clone(), Vec::new());
        let record = snapshot
            .records
            .iter()
            .find(|record| record.client == ClientKind::Codex && record.name == "rollback")
            .unwrap();

        let error = set_inventory_record_enabled_with_writer(
            home,
            codex_home,
            Vec::new(),
            InventoryActionRequest {
                record_id: record.id.clone(),
                enabled: false,
                source_revision: record.action_capabilities.source_revision.clone().unwrap(),
            },
            &SkillMutatingWriter { skill_file },
        )
        .unwrap_err();

        assert_eq!(error, InventoryActionError::StaleInventory);
        assert_eq!(fs::read(config_path).unwrap(), original);
    }
}
