use super::actions::InventoryActionError;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

mod platform;

pub(super) struct ConfigSource {
    pub(super) target_path: PathBuf,
    pub(super) contents: Option<Vec<u8>>,
    #[cfg(target_os = "macos")]
    identity: Option<platform::FileIdentity>,
}

impl ConfigSource {
    pub(super) fn read(config_path: &Path) -> Result<Self, InventoryActionError> {
        match fs::symlink_metadata(config_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Err(InventoryActionError::UnsafeConfiguration)
            }
            Ok(metadata) if metadata.is_file() => {
                #[cfg(target_os = "macos")]
                let (contents, identity) = platform::read_regular_file(config_path)
                    .map_err(|_| InventoryActionError::UnsafeConfiguration)?;
                #[cfg(not(target_os = "macos"))]
                let contents =
                    fs::read(config_path).map_err(|_| InventoryActionError::UnsafeConfiguration)?;
                Ok(Self {
                    target_path: config_path.to_path_buf(),
                    contents: Some(contents),
                    #[cfg(target_os = "macos")]
                    identity: Some(identity),
                })
            }
            Ok(_) => Err(InventoryActionError::UnsafeConfiguration),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self {
                target_path: config_path.to_path_buf(),
                contents: None,
                #[cfg(target_os = "macos")]
                identity: None,
            }),
            Err(_) => Err(InventoryActionError::UnsafeConfiguration),
        }
    }

    fn validate_config_entry(&self) -> Result<(), ConfigWriteError> {
        #[cfg(target_os = "macos")]
        match self.identity {
            Some(identity) if !platform::path_matches_identity(&self.target_path, identity) => {
                return Err(ConfigWriteError::SourceChanged);
            }
            None if fs::symlink_metadata(&self.target_path).is_ok() => {
                return Err(ConfigWriteError::SourceChanged);
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConfigWriteError {
    SourceChanged,
    Io,
    RollbackFailed,
}

pub(super) trait ConfigWriter {
    fn replace(&self, source: &ConfigSource, updated: &[u8]) -> Result<(), ConfigWriteError>;
}

pub(super) struct AtomicConfigWriter;

impl ConfigWriter for AtomicConfigWriter {
    fn replace(&self, source: &ConfigSource, updated: &[u8]) -> Result<(), ConfigWriteError> {
        source.validate_config_entry()?;
        #[cfg(target_os = "macos")]
        let current = match source.identity {
            Some(expected) => {
                let (contents, actual) = platform::read_regular_file(&source.target_path)?;
                if actual != expected {
                    return Err(ConfigWriteError::SourceChanged);
                }
                Some(contents)
            }
            None => None,
        };
        #[cfg(not(target_os = "macos"))]
        let current = match fs::read(&source.target_path) {
            Ok(contents) => Some(contents),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(_) => return Err(ConfigWriteError::Io),
        };
        if current.as_deref() != source.contents.as_deref() {
            return Err(ConfigWriteError::SourceChanged);
        }
        source.validate_config_entry()?;
        let parent = source.target_path.parent().ok_or(ConfigWriteError::Io)?;
        fs::create_dir_all(parent).map_err(|_| ConfigWriteError::Io)?;
        let temp_path = sidecar_path(&source.target_path, "tmp")?;
        write_and_replace(&temp_path, &source.target_path, source, updated)
    }
}

fn write_and_replace(
    temp_path: &Path,
    target_path: &Path,
    source: &ConfigSource,
    updated: &[u8],
) -> Result<(), ConfigWriteError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut temp_file = options.open(temp_path).map_err(|_| ConfigWriteError::Io)?;
    let temp_identity = platform::file_identity_from_file(&temp_file)?;
    let result = (|| {
        temp_file
            .write_all(updated)
            .and_then(|_| temp_file.sync_all())
            .map_err(|_| ConfigWriteError::Io)?;
        drop(temp_file);
        if source.contents.is_some() {
            source.validate_config_entry()?;
            replace_existing_guarded(temp_path, target_path, source, updated, temp_identity)?;
        } else {
            platform::commit_new(temp_path, target_path)?;
        }
        sync_parent(target_path);
        Ok(())
    })();
    if matches!(&result, Err(error) if *error != ConfigWriteError::RollbackFailed) {
        let _ = platform::remove_if_identity(temp_path, temp_identity);
    }
    result
}

fn replace_existing_guarded(
    temp_path: &Path,
    target_path: &Path,
    source: &ConfigSource,
    updated: &[u8],
    temp_identity: platform::FileIdentity,
) -> Result<(), ConfigWriteError> {
    let backup_path = sidecar_path(target_path, "backup")?;
    #[cfg(target_os = "macos")]
    let source_identity = source.identity.ok_or(ConfigWriteError::SourceChanged)?;
    #[cfg(not(target_os = "macos"))]
    let source_identity = platform::file_identity(target_path)?;
    platform::replace_existing(
        target_path,
        temp_path,
        &backup_path,
        source_identity,
        temp_identity,
    )?;
    let (replaced, backup_identity) = match platform::read_regular_file(&backup_path) {
        Ok(replaced) => replaced,
        Err(_) => {
            rollback_if_update_is_current(target_path, &backup_path, updated)?;
            return Err(ConfigWriteError::RollbackFailed);
        }
    };
    if backup_identity != source_identity {
        rollback_if_update_is_current(target_path, &backup_path, updated)?;
        return Err(ConfigWriteError::SourceChanged);
    }
    if source.contents.as_deref() == Some(replaced.as_slice()) {
        if platform::copy_security_metadata_guarded(
            &backup_path,
            source_identity,
            target_path,
            temp_identity,
        )
        .is_err()
        {
            rollback_if_update_is_current(target_path, &backup_path, updated)?;
            return Err(ConfigWriteError::Io);
        }
        if !platform::path_matches_identity(target_path, temp_identity) {
            rollback_if_update_is_current(target_path, &backup_path, updated)?;
            return Err(ConfigWriteError::SourceChanged);
        }
        if platform::remove_if_identity(&backup_path, source_identity).is_err() {
            rollback_if_update_is_current(target_path, &backup_path, updated)?;
            return Err(ConfigWriteError::Io);
        }
        return Ok(());
    }

    rollback_if_update_is_current(target_path, &backup_path, updated)?;
    sync_parent(target_path);
    Err(ConfigWriteError::SourceChanged)
}

fn sidecar_path(target_path: &Path, suffix: &str) -> Result<PathBuf, ConfigWriteError> {
    let file_name = target_path.file_name().ok_or(ConfigWriteError::Io)?;
    let mut sidecar_name = OsString::from(".");
    sidecar_name.push(file_name);
    sidecar_name.push(".");
    sidecar_name.push(Uuid::new_v4().to_string());
    sidecar_name.push(".");
    sidecar_name.push(suffix);
    Ok(target_path.with_file_name(sidecar_name))
}

fn rollback_if_update_is_current(
    target_path: &Path,
    backup_path: &Path,
    updated: &[u8],
) -> Result<(), ConfigWriteError> {
    let quarantine_path = sidecar_path(target_path, "rollback-current")?;
    let result =
        platform::restore_backup_if_matches(target_path, backup_path, updated, &quarantine_path);
    sync_parent(target_path);
    result
}

fn sync_parent(target_path: &Path) {
    #[cfg(unix)]
    if let Some(parent) = target_path.parent() {
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
    }
}

pub(super) fn restore_original(
    source: &ConfigSource,
    written: &[u8],
) -> Result<(), InventoryActionError> {
    match source.contents.as_deref() {
        Some(original) => {
            let written_source = ConfigSource::read(&source.target_path)
                .map_err(|_| InventoryActionError::RollbackFailed)?;
            if written_source.contents.as_deref() != Some(written) {
                return Err(InventoryActionError::RollbackFailed);
            }
            AtomicConfigWriter
                .replace(&written_source, original)
                .map_err(|_| InventoryActionError::RollbackFailed)
        }
        None => {
            let quarantine_path = sidecar_path(&source.target_path, "rollback")
                .map_err(|_| InventoryActionError::RollbackFailed)?;
            let result = platform::guarded_remove(&source.target_path, written, &quarantine_path);
            sync_parent(&source.target_path);
            result.map_err(|_| InventoryActionError::RollbackFailed)?;
            Ok(())
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn restores_existing_config_after_a_failed_read_back() {
        let fixture = TempDir::new().unwrap();
        let config_path = fixture.path().join("config.toml");
        fs::write(&config_path, "# original\n").unwrap();
        let source = ConfigSource::read(&config_path).unwrap();
        let updated = b"# updated\n";
        AtomicConfigWriter.replace(&source, updated).unwrap();

        restore_original(&source, updated).unwrap();

        assert_eq!(fs::read(config_path).unwrap(), b"# original\n");
    }

    #[test]
    fn removes_new_config_after_a_failed_read_back() {
        let fixture = TempDir::new().unwrap();
        let config_path = fixture.path().join("codex/config.toml");
        let source = ConfigSource::read(&config_path).unwrap();
        let updated = b"# newly created\n";
        AtomicConfigWriter.replace(&source, updated).unwrap();

        restore_original(&source, updated).unwrap();

        assert!(!config_path.exists());
    }

    #[test]
    fn keeps_a_concurrent_config_created_before_new_config_rollback() {
        let fixture = TempDir::new().unwrap();
        let config_path = fixture.path().join("codex/config.toml");
        let source = ConfigSource::read(&config_path).unwrap();
        let updated = b"# newly created\n";
        AtomicConfigWriter.replace(&source, updated).unwrap();
        fs::write(&config_path, "# concurrent config\n").unwrap();

        let error = restore_original(&source, updated).unwrap_err();

        assert_eq!(error, InventoryActionError::RollbackFailed);
        assert_eq!(fs::read(config_path).unwrap(), b"# concurrent config\n");
    }

    #[test]
    fn refuses_to_overwrite_bytes_changed_after_validation() {
        let fixture = TempDir::new().unwrap();
        let config_path = fixture.path().join("config.toml");
        fs::write(&config_path, "# scanned\n").unwrap();
        let source = ConfigSource::read(&config_path).unwrap();
        fs::write(&config_path, "# changed elsewhere\n").unwrap();

        let error = AtomicConfigWriter
            .replace(&source, b"# requested\n")
            .unwrap_err();

        assert_eq!(error, ConfigWriteError::SourceChanged);
        assert_eq!(fs::read(config_path).unwrap(), b"# changed elsewhere\n");
    }

    #[test]
    fn restores_a_concurrent_edit_that_lands_while_the_replacement_is_prepared() {
        let fixture = TempDir::new().unwrap();
        let config_path = fixture.path().join("config.toml");
        let temp_path = fixture.path().join(".config.concurrent.tmp");
        fs::write(&config_path, "# scanned\n").unwrap();
        let source = ConfigSource::read(&config_path).unwrap();
        fs::write(&config_path, "# concurrent edit\n").unwrap();

        let error =
            write_and_replace(&temp_path, &config_path, &source, b"# requested\n").unwrap_err();

        assert_eq!(error, ConfigWriteError::SourceChanged);
        assert_eq!(fs::read(config_path).unwrap(), b"# concurrent edit\n");
    }

    #[test]
    fn restores_a_backup_only_when_the_current_config_is_our_update() {
        let fixture = TempDir::new().unwrap();
        let config_path = fixture.path().join("config.toml");
        let backup_path = fixture.path().join(".config.backup");
        let quarantine_path = fixture.path().join(".config.rollback-current");
        fs::write(&config_path, "# requested\n").unwrap();
        fs::write(&backup_path, "# original\n").unwrap();

        platform::restore_backup_if_matches(
            &config_path,
            &backup_path,
            b"# requested\n",
            &quarantine_path,
        )
        .unwrap();

        assert_eq!(fs::read(config_path).unwrap(), b"# original\n");
        assert!(!backup_path.exists());
        assert!(!quarantine_path.exists());
    }

    #[test]
    fn never_replaces_a_newer_config_during_backup_restore() {
        let fixture = TempDir::new().unwrap();
        let config_path = fixture.path().join("config.toml");
        let backup_path = fixture.path().join(".config.backup");
        let quarantine_path = fixture.path().join(".config.rollback-current");
        fs::write(&config_path, "# newer save\n").unwrap();
        fs::write(&backup_path, "# original\n").unwrap();

        let error = platform::restore_backup_if_matches(
            &config_path,
            &backup_path,
            b"# requested\n",
            &quarantine_path,
        )
        .unwrap_err();

        assert_eq!(error, ConfigWriteError::RollbackFailed);
        assert_eq!(fs::read(config_path).unwrap(), b"# newer save\n");
        assert!(!backup_path.exists());
        assert!(!quarantine_path.exists());
    }

    #[test]
    fn retains_the_displaced_backup_when_another_save_wins_the_restore_race() {
        let fixture = TempDir::new().unwrap();
        let config_path = fixture.path().join("config.toml");
        let backup_path = fixture.path().join(".config.backup");
        let quarantine_path = fixture.path().join(".config.rollback-current");
        fs::write(&config_path, "# newest save\n").unwrap();
        fs::write(&backup_path, "# displaced concurrent edit\n").unwrap();
        fs::write(&quarantine_path, "# requested update\n").unwrap();

        let error = platform::restore_matching_backup(&config_path, &backup_path, &quarantine_path)
            .unwrap_err();

        assert_eq!(error, ConfigWriteError::RollbackFailed);
        assert_eq!(fs::read(config_path).unwrap(), b"# newest save\n");
        assert_eq!(
            fs::read(backup_path).unwrap(),
            b"# displaced concurrent edit\n"
        );
        assert!(!quarantine_path.exists());
    }

    #[test]
    fn rejects_config_symlinks_before_writing() {
        use std::os::unix::fs::symlink;

        let fixture = TempDir::new().unwrap();
        let original_target = fixture.path().join("original.toml");
        let config_path = fixture.path().join("config.toml");
        fs::write(&original_target, "# original\n").unwrap();
        symlink(&original_target, &config_path).unwrap();

        let result = ConfigSource::read(&config_path);

        assert!(matches!(
            result,
            Err(InventoryActionError::UnsafeConfiguration)
        ));
        assert_eq!(fs::read(original_target).unwrap(), b"# original\n");
    }

    #[test]
    fn rejects_a_config_replaced_by_a_symlink_after_validation() {
        use std::os::unix::fs::symlink;

        let fixture = TempDir::new().unwrap();
        let config_path = fixture.path().join("config.toml");
        let newer_target = fixture.path().join("newer.toml");
        fs::write(&config_path, "# original\n").unwrap();
        fs::write(&newer_target, "# newer\n").unwrap();
        let source = ConfigSource::read(&config_path).unwrap();
        fs::remove_file(&config_path).unwrap();
        symlink(&newer_target, &config_path).unwrap();

        let error = AtomicConfigWriter
            .replace(&source, b"# requested\n")
            .unwrap_err();

        assert_eq!(error, ConfigWriteError::SourceChanged);
        assert_eq!(fs::read(newer_target).unwrap(), b"# newer\n");
    }

    #[test]
    fn preserves_both_files_when_backup_staging_fails() {
        let fixture = TempDir::new().unwrap();
        let config_path = fixture.path().join("config.toml");
        let replacement_path = fixture.path().join(".config.prepared.tmp");
        let backup_path = fixture.path().join("occupied-backup");
        fs::write(&config_path, "# original\n").unwrap();
        fs::write(&replacement_path, "# replacement\n").unwrap();
        fs::create_dir(&backup_path).unwrap();

        let target_identity = platform::file_identity(&config_path).unwrap();
        let replacement_identity = platform::file_identity(&replacement_path).unwrap();
        let error = platform::replace_existing(
            &config_path,
            &replacement_path,
            &backup_path,
            target_identity,
            replacement_identity,
        )
        .unwrap_err();

        assert_eq!(error, ConfigWriteError::RollbackFailed);
        assert_eq!(fs::read(config_path).unwrap(), b"# replacement\n");
        assert_eq!(fs::read(replacement_path).unwrap(), b"# original\n");
        assert!(backup_path.is_dir());
    }

    #[test]
    fn keeps_quarantined_config_when_another_save_wins_the_restore_race() {
        let fixture = TempDir::new().unwrap();
        let config_path = fixture.path().join("config.toml");
        let quarantine_path = fixture.path().join(".config.rollback");
        fs::write(&config_path, "# newest save\n").unwrap();
        fs::write(&quarantine_path, "# quarantined save\n").unwrap();

        let error = platform::restore_quarantine_without_overwrite(&quarantine_path, &config_path)
            .unwrap_err();

        assert_eq!(error, ConfigWriteError::RollbackFailed);
        assert_eq!(fs::read(config_path).unwrap(), b"# newest save\n");
        assert_eq!(fs::read(quarantine_path).unwrap(), b"# quarantined save\n");
    }

    #[cfg(unix)]
    #[test]
    fn preserves_extended_security_metadata() {
        let fixture = TempDir::new().unwrap();
        let config_path = fixture.path().join("config.toml");
        fs::write(&config_path, "# original\n").unwrap();
        let attribute = if cfg!(target_os = "macos") {
            "com.plaintext-lab.inventory-test"
        } else {
            "user.inventory-test"
        };
        xattr::set(&config_path, attribute, b"protected").unwrap();
        let source = ConfigSource::read(&config_path).unwrap();

        AtomicConfigWriter
            .replace(&source, b"# replacement\n")
            .unwrap();

        assert_eq!(
            xattr::get(config_path, attribute).unwrap().unwrap(),
            b"protected"
        );
    }

    #[test]
    fn preserves_metadata_changed_after_the_replacement_is_prepared() {
        let fixture = TempDir::new().unwrap();
        let config_path = fixture.path().join("config.toml");
        let temp_path = fixture.path().join(".config.prepared.tmp");
        let attribute = "com.plaintext-lab.inventory-race-test";
        fs::write(&config_path, "# original\n").unwrap();
        fs::write(&temp_path, "# replacement\n").unwrap();
        xattr::set(&config_path, attribute, b"scanned").unwrap();
        let source = ConfigSource::read(&config_path).unwrap();
        xattr::set(&config_path, attribute, b"changed-after-copy").unwrap();

        let temp_identity = platform::file_identity(&temp_path).unwrap();
        replace_existing_guarded(
            &temp_path,
            &config_path,
            &source,
            b"# replacement\n",
            temp_identity,
        )
        .unwrap();

        assert_eq!(fs::read(&config_path).unwrap(), b"# replacement\n");
        assert_eq!(
            xattr::get(config_path, attribute).unwrap().unwrap(),
            b"changed-after-copy"
        );
    }

    #[test]
    fn never_copies_metadata_to_a_newer_target_file() {
        let fixture = TempDir::new().unwrap();
        let source_path = fixture.path().join("source.toml");
        let destination_path = fixture.path().join("destination.toml");
        let displaced_path = fixture.path().join("displaced.toml");
        let attribute = "com.plaintext-lab.inventory-target-race-test";
        fs::write(&source_path, "# source\n").unwrap();
        fs::write(&destination_path, "# prepared\n").unwrap();
        xattr::set(&source_path, attribute, b"protected").unwrap();
        let source_identity = platform::file_identity(&source_path).unwrap();
        let destination_identity = platform::file_identity(&destination_path).unwrap();
        fs::rename(&destination_path, &displaced_path).unwrap();
        fs::write(&destination_path, "# concurrent\n").unwrap();

        let error = platform::copy_security_metadata_guarded(
            &source_path,
            source_identity,
            &destination_path,
            destination_identity,
        )
        .unwrap_err();

        assert_eq!(error, ConfigWriteError::SourceChanged);
        assert_eq!(fs::read(destination_path).unwrap(), b"# concurrent\n");
        assert_eq!(xattr::get(displaced_path, attribute).unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn creates_a_sidecar_name_for_a_non_utf8_config_target() {
        use std::ffi::OsString;
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let target_name = OsString::from_vec(b"config-\xff.toml".to_vec());
        let config_target = PathBuf::from(target_name);

        let sidecar = sidecar_path(&config_target, "tmp").unwrap();

        assert!(sidecar
            .file_name()
            .unwrap()
            .as_bytes()
            .windows(b"config-\xff.toml".len())
            .any(|window| window == b"config-\xff.toml"));
    }

    #[test]
    fn atomically_replaces_an_existing_config() {
        let fixture = TempDir::new().unwrap();
        let config_path = fixture.path().join("config.toml");
        fs::write(&config_path, "# original\n").unwrap();
        let source = ConfigSource::read(&config_path).unwrap();

        AtomicConfigWriter
            .replace(&source, b"# replacement\n")
            .unwrap();

        assert_eq!(fs::read(config_path).unwrap(), b"# replacement\n");
    }
}
