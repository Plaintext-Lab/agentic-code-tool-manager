use super::actions::InventoryActionError;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub(super) struct ConfigSource {
    pub(super) target_path: PathBuf,
    pub(super) contents: Option<Vec<u8>>,
    permissions: Option<fs::Permissions>,
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
                    permissions: Some(target_metadata.permissions()),
                })
            }
            Ok(metadata) if metadata.is_file() => {
                let contents =
                    fs::read(config_path).map_err(|_| InventoryActionError::UnsafeConfiguration)?;
                Ok(Self {
                    target_path: config_path.to_path_buf(),
                    contents: Some(contents),
                    permissions: Some(metadata.permissions()),
                })
            }
            Ok(_) => Err(InventoryActionError::UnsafeConfiguration),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self {
                target_path: config_path.to_path_buf(),
                contents: None,
                permissions: None,
            }),
            Err(_) => Err(InventoryActionError::UnsafeConfiguration),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConfigWriteError {
    SourceChanged,
    Io,
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
        let file_name = source
            .target_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(ConfigWriteError::Io)?;
        let temp_path = parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
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
    if let Some(permissions) = source.permissions.clone() {
        temp_file
            .set_permissions(permissions)
            .map_err(|_| ConfigWriteError::Io)?;
    }
    replace_file(temp_path, target_path)?;
    sync_parent(target_path);
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(temp_path: &Path, target_path: &Path) -> Result<(), ConfigWriteError> {
    fs::rename(temp_path, target_path).map_err(|_| ConfigWriteError::Io)
}

#[cfg(windows)]
fn replace_file(temp_path: &Path, target_path: &Path) -> Result<(), ConfigWriteError> {
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = temp_path.as_os_str().encode_wide().chain(once(0)).collect();
    let destination: Vec<u16> = target_path
        .as_os_str()
        .encode_wide()
        .chain(once(0))
        .collect();
    // SAFETY: Both pointers reference NUL-terminated buffers that remain alive for the call.
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        return Err(ConfigWriteError::Io);
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
                permissions: source.permissions.clone(),
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

#[cfg(test)]
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
