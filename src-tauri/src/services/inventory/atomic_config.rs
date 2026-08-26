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
                Ok(Self {
                    target_path,
                    contents: Some(contents),
                })
            }
            Ok(metadata) if metadata.is_file() => {
                let contents =
                    fs::read(config_path).map_err(|_| InventoryActionError::UnsafeConfiguration)?;
                Ok(Self {
                    target_path: config_path.to_path_buf(),
                    contents: Some(contents),
                })
            }
            Ok(_) => Err(InventoryActionError::UnsafeConfiguration),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self {
                target_path: config_path.to_path_buf(),
                contents: None,
            }),
            Err(_) => Err(InventoryActionError::UnsafeConfiguration),
        }
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
        let current = match fs::read(&source.target_path) {
            Ok(contents) => Some(contents),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(_) => return Err(ConfigWriteError::Io),
        };
        if current.as_deref() != source.contents.as_deref() {
            return Err(ConfigWriteError::SourceChanged);
        }
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
        replace_existing_guarded(temp_path, target_path, source, updated)?;
    } else {
        match fs::hard_link(temp_path, target_path) {
            Ok(()) => fs::remove_file(temp_path).map_err(|_| ConfigWriteError::Io)?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(ConfigWriteError::SourceChanged);
            }
            Err(_) => return Err(ConfigWriteError::Io),
        }
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
    let target_still_contains_update = fs::read(target_path)
        .map(|contents| contents == updated)
        .unwrap_or(false);
    if target_still_contains_update {
        platform::restore_backup(target_path, backup_path)?;
    }
    if backup_path.exists() {
        fs::remove_file(backup_path).map_err(|_| ConfigWriteError::RollbackFailed)?;
    }
    Ok(())
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
            };
            AtomicConfigWriter
                .replace(&written_source, original)
                .map_err(|_| InventoryActionError::RollbackFailed)
        }
        None => {
            let current =
                fs::read(&source.target_path).map_err(|_| InventoryActionError::RollbackFailed)?;
            if current != written {
                return Err(InventoryActionError::RollbackFailed);
            }
            fs::remove_file(&source.target_path)
                .map_err(|_| InventoryActionError::RollbackFailed)?;
            sync_parent(&source.target_path);
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
