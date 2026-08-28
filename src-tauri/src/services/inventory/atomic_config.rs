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
    symlink_guard: Option<ConfigSymlinkGuard>,
}

#[cfg(target_os = "macos")]
#[derive(Clone)]
struct ConfigSymlinkGuard {
    link_path: PathBuf,
    link_target: PathBuf,
    resolved_target: PathBuf,
    device: u64,
    inode: u64,
}

#[cfg(target_os = "macos")]
impl ConfigSymlinkGuard {
    fn capture(
        link_path: &Path,
        metadata: &fs::Metadata,
        resolved_target: PathBuf,
    ) -> Result<Self, InventoryActionError> {
        use std::os::unix::fs::MetadataExt;

        let link_target =
            fs::read_link(link_path).map_err(|_| InventoryActionError::UnsafeConfiguration)?;
        Ok(Self {
            link_path: link_path.to_path_buf(),
            link_target,
            resolved_target,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    fn is_current(&self) -> bool {
        use std::os::unix::fs::MetadataExt;

        let Ok(metadata) = fs::symlink_metadata(&self.link_path) else {
            return false;
        };
        metadata.file_type().is_symlink()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
            && fs::read_link(&self.link_path).is_ok_and(|target| target == self.link_target)
            && fs::canonicalize(&self.link_path).is_ok_and(|target| target == self.resolved_target)
    }
}

impl ConfigSource {
    pub(super) fn read(config_path: &Path) -> Result<Self, InventoryActionError> {
        match fs::symlink_metadata(config_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let target_path = fs::canonicalize(config_path)
                    .map_err(|_| InventoryActionError::UnsafeConfiguration)?;
                let target_metadata = fs::metadata(&target_path)
                    .map_err(|_| InventoryActionError::UnsafeConfiguration)?;
                if !target_metadata.is_file() {
                    return Err(InventoryActionError::UnsafeConfiguration);
                }
                let contents = fs::read(&target_path)
                    .map_err(|_| InventoryActionError::UnsafeConfiguration)?;
                #[cfg(target_os = "macos")]
                let symlink_guard = Some(ConfigSymlinkGuard::capture(
                    config_path,
                    &metadata,
                    target_path.clone(),
                )?);
                let source = Self {
                    target_path,
                    contents: Some(contents),
                    #[cfg(target_os = "macos")]
                    symlink_guard,
                };
                source
                    .validate_config_entry()
                    .map_err(|_| InventoryActionError::UnsafeConfiguration)?;
                Ok(source)
            }
            Ok(metadata) if metadata.is_file() => {
                let contents =
                    fs::read(config_path).map_err(|_| InventoryActionError::UnsafeConfiguration)?;
                Ok(Self {
                    target_path: config_path.to_path_buf(),
                    contents: Some(contents),
                    #[cfg(target_os = "macos")]
                    symlink_guard: None,
                })
            }
            Ok(_) => Err(InventoryActionError::UnsafeConfiguration),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self {
                target_path: config_path.to_path_buf(),
                contents: None,
                #[cfg(target_os = "macos")]
                symlink_guard: None,
            }),
            Err(_) => Err(InventoryActionError::UnsafeConfiguration),
        }
    }

    fn validate_config_entry(&self) -> Result<(), ConfigWriteError> {
        #[cfg(target_os = "macos")]
        if self
            .symlink_guard
            .as_ref()
            .is_some_and(|guard| !guard.is_current())
        {
            return Err(ConfigWriteError::SourceChanged);
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
        let result = write_and_replace(&temp_path, &source.target_path, source, updated);
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result
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
    temp_file
        .write_all(updated)
        .and_then(|_| temp_file.sync_all())
        .map_err(|_| ConfigWriteError::Io)?;
    drop(temp_file);
    if source.contents.is_some() {
        platform::copy_security_metadata(target_path, temp_path)?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(temp_path)
            .and_then(|file| file.sync_all())
            .map_err(|_| ConfigWriteError::Io)?;
        source.validate_config_entry()?;
        replace_existing_guarded(temp_path, target_path, source, updated)?;
    } else {
        platform::commit_new(temp_path, target_path)?;
    }
    sync_parent(target_path);
    Ok(())
}

fn replace_existing_guarded(
    temp_path: &Path,
    target_path: &Path,
    source: &ConfigSource,
    updated: &[u8],
) -> Result<(), ConfigWriteError> {
    let backup_path = sidecar_path(target_path, "backup")?;
    platform::replace_existing(target_path, temp_path, &backup_path)?;
    let replaced = match fs::read(&backup_path) {
        Ok(replaced) => replaced,
        Err(_) => {
            rollback_if_update_is_current(target_path, &backup_path, updated)?;
            return Err(ConfigWriteError::RollbackFailed);
        }
    };
    if source.contents.as_deref() == Some(replaced.as_slice()) {
        if platform::copy_security_metadata(&backup_path, target_path).is_err()
            || fs::File::open(target_path)
                .and_then(|file| file.sync_all())
                .is_err()
        {
            rollback_if_update_is_current(target_path, &backup_path, updated)?;
            return Err(ConfigWriteError::Io);
        }
        if let Err(error) = source.validate_config_entry() {
            rollback_if_update_is_current(target_path, &backup_path, updated)?;
            return Err(error);
        }
        if fs::remove_file(&backup_path).is_err() {
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
            let written_source = ConfigSource {
                target_path: source.target_path.clone(),
                contents: Some(written.to_vec()),
                #[cfg(target_os = "macos")]
                symlink_guard: None,
            };
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
    fn rejects_a_config_symlink_retargeted_after_validation() {
        use std::os::unix::fs::symlink;

        let fixture = TempDir::new().unwrap();
        let original_target = fixture.path().join("original.toml");
        let newer_target = fixture.path().join("newer.toml");
        let config_path = fixture.path().join("config.toml");
        fs::write(&original_target, "# original\n").unwrap();
        fs::write(&newer_target, "# newer\n").unwrap();
        symlink(&original_target, &config_path).unwrap();
        let source = ConfigSource::read(&config_path).unwrap();
        fs::remove_file(&config_path).unwrap();
        symlink(&newer_target, &config_path).unwrap();

        let error = AtomicConfigWriter
            .replace(&source, b"# requested\n")
            .unwrap_err();

        assert_eq!(error, ConfigWriteError::SourceChanged);
        assert_eq!(fs::read(original_target).unwrap(), b"# original\n");
        assert_eq!(fs::read(newer_target).unwrap(), b"# newer\n");
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
        platform::copy_security_metadata(&config_path, &temp_path).unwrap();
        xattr::set(&config_path, attribute, b"changed-after-copy").unwrap();

        replace_existing_guarded(&temp_path, &config_path, &source, b"# replacement\n").unwrap();

        assert_eq!(fs::read(&config_path).unwrap(), b"# replacement\n");
        assert_eq!(
            xattr::get(config_path, attribute).unwrap().unwrap(),
            b"changed-after-copy"
        );
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
