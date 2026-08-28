use super::ConfigWriteError;
use std::fs::File;
#[cfg(target_os = "macos")]
use std::io::Read;
use std::path::Path;

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FileIdentity {
    device: u64,
    inode: u64,
}

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FileIdentity;

#[cfg(target_os = "macos")]
fn identity_from_metadata(metadata: &std::fs::Metadata) -> FileIdentity {
    use std::os::unix::fs::MetadataExt;

    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

#[cfg(target_os = "macos")]
fn is_exclusive_regular_file(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.is_file() && metadata.nlink() == 1
}

#[cfg(target_os = "macos")]
pub(super) fn file_identity(path: &Path) -> Result<FileIdentity, ConfigWriteError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| ConfigWriteError::Io)?;
    if !is_exclusive_regular_file(&metadata) {
        return Err(ConfigWriteError::SourceChanged);
    }
    Ok(identity_from_metadata(&metadata))
}

#[cfg(target_os = "macos")]
pub(super) fn file_identity_from_file(file: &File) -> Result<FileIdentity, ConfigWriteError> {
    let metadata = file.metadata().map_err(|_| ConfigWriteError::Io)?;
    if !is_exclusive_regular_file(&metadata) {
        return Err(ConfigWriteError::SourceChanged);
    }
    Ok(identity_from_metadata(&metadata))
}

#[cfg(target_os = "macos")]
pub(super) fn path_matches_identity(path: &Path, expected: FileIdentity) -> bool {
    file_identity(path).is_ok_and(|actual| actual == expected)
}

#[cfg(target_os = "macos")]
pub(super) fn read_regular_file(path: &Path) -> Result<(Vec<u8>, FileIdentity), ConfigWriteError> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| ConfigWriteError::SourceChanged)?;
    let identity = file_identity_from_file(&file)?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)
        .map_err(|_| ConfigWriteError::Io)?;
    Ok((contents, identity))
}

#[cfg(target_os = "macos")]
pub(super) fn copy_security_metadata_guarded(
    source: &Path,
    expected_source: FileIdentity,
    destination: &Path,
    expected_destination: FileIdentity,
) -> Result<(), ConfigWriteError> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;

    let source_file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(source)
        .map_err(|_| ConfigWriteError::SourceChanged)?;
    let destination_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(destination)
        .map_err(|_| ConfigWriteError::SourceChanged)?;
    if file_identity_from_file(&source_file)? != expected_source
        || file_identity_from_file(&destination_file)? != expected_destination
    {
        return Err(ConfigWriteError::SourceChanged);
    }
    // SAFETY: Both descriptors remain open for the duration of the call.
    let result = unsafe {
        libc::fcopyfile(
            source_file.as_raw_fd(),
            destination_file.as_raw_fd(),
            std::ptr::null_mut(),
            libc::COPYFILE_METADATA,
        )
    };
    if result != 0 {
        return Err(ConfigWriteError::Io);
    }
    destination_file
        .sync_all()
        .map_err(|_| ConfigWriteError::Io)?;
    if !path_matches_identity(destination, expected_destination) {
        return Err(ConfigWriteError::SourceChanged);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn replace_existing(
    target: &Path,
    replacement: &Path,
    backup: &Path,
    expected_target: FileIdentity,
    expected_replacement: FileIdentity,
) -> Result<(), ConfigWriteError> {
    if !path_matches_identity(target, expected_target)
        || !path_matches_identity(replacement, expected_replacement)
    {
        return Err(ConfigWriteError::SourceChanged);
    }
    exchange_files(replacement, target)?;
    if !path_matches_identity(target, expected_replacement)
        || !path_matches_identity(replacement, expected_target)
    {
        return Err(ConfigWriteError::RollbackFailed);
    }
    if std::fs::rename(replacement, backup).is_err() {
        return Err(ConfigWriteError::RollbackFailed);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn remove_if_identity(
    path: &Path,
    expected: FileIdentity,
) -> Result<(), ConfigWriteError> {
    if !path_matches_identity(path, expected) {
        return Err(ConfigWriteError::SourceChanged);
    }
    std::fs::remove_file(path).map_err(|_| ConfigWriteError::Io)
}

#[cfg(target_os = "macos")]
pub(super) fn commit_new(replacement: &Path, target: &Path) -> Result<(), ConfigWriteError> {
    rename_exclusive(replacement, target)
}

#[cfg(target_os = "macos")]
pub(super) fn restore_backup_if_matches(
    target: &Path,
    backup: &Path,
    expected: &[u8],
    quarantine: &Path,
) -> Result<(), ConfigWriteError> {
    std::fs::rename(target, quarantine).map_err(|_| ConfigWriteError::RollbackFailed)?;
    let displaced_matches = std::fs::read(quarantine)
        .as_deref()
        .is_ok_and(|contents| contents == expected);

    if displaced_matches {
        return restore_matching_backup(target, backup, quarantine);
    }

    match restore_quarantine_without_overwrite(quarantine, target) {
        Ok(()) => {
            std::fs::remove_file(backup).map_err(|_| ConfigWriteError::RollbackFailed)?;
        }
        Err(error) => return Err(error),
    }
    Err(ConfigWriteError::RollbackFailed)
}

#[cfg(target_os = "macos")]
pub(super) fn restore_matching_backup(
    target: &Path,
    backup: &Path,
    quarantine: &Path,
) -> Result<(), ConfigWriteError> {
    match rename_exclusive(backup, target) {
        Ok(()) => std::fs::remove_file(quarantine).map_err(|_| ConfigWriteError::RollbackFailed),
        Err(ConfigWriteError::SourceChanged) => {
            let _ = std::fs::remove_file(quarantine);
            Err(ConfigWriteError::RollbackFailed)
        }
        Err(_) => {
            let _ = rename_exclusive(quarantine, target);
            Err(ConfigWriteError::RollbackFailed)
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn restore_quarantine_without_overwrite(
    quarantine: &Path,
    target: &Path,
) -> Result<(), ConfigWriteError> {
    rename_exclusive(quarantine, target).map_err(|_| ConfigWriteError::RollbackFailed)
}

#[cfg(target_os = "macos")]
pub(super) fn guarded_remove(
    target: &Path,
    expected: &[u8],
    quarantine: &Path,
) -> Result<(), ConfigWriteError> {
    std::fs::rename(target, quarantine).map_err(|_| ConfigWriteError::RollbackFailed)?;
    let displaced = std::fs::read(quarantine);
    if displaced
        .as_deref()
        .is_ok_and(|contents| contents == expected)
    {
        return std::fs::remove_file(quarantine).map_err(|_| ConfigWriteError::RollbackFailed);
    }

    restore_quarantine_without_overwrite(quarantine, target)?;
    Err(ConfigWriteError::RollbackFailed)
}

#[cfg(target_os = "macos")]
fn exchange_files(left: &Path, right: &Path) -> Result<(), ConfigWriteError> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let left = CString::new(left.as_os_str().as_bytes()).map_err(|_| ConfigWriteError::Io)?;
    let right = CString::new(right.as_os_str().as_bytes()).map_err(|_| ConfigWriteError::Io)?;
    // SAFETY: Both C strings are NUL-terminated and remain alive for the call.
    let result = unsafe { libc::renamex_np(left.as_ptr(), right.as_ptr(), libc::RENAME_SWAP) };
    if result == 0 {
        Ok(())
    } else {
        Err(ConfigWriteError::Io)
    }
}

#[cfg(target_os = "macos")]
fn rename_exclusive(source: &Path, destination: &Path) -> Result<(), ConfigWriteError> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_os_str().as_bytes()).map_err(|_| ConfigWriteError::Io)?;
    let destination =
        CString::new(destination.as_os_str().as_bytes()).map_err(|_| ConfigWriteError::Io)?;
    // SAFETY: Both C strings are NUL-terminated and remain alive for the call.
    let result =
        unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        Err(ConfigWriteError::SourceChanged)
    } else {
        Err(ConfigWriteError::Io)
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn file_identity(_path: &Path) -> Result<FileIdentity, ConfigWriteError> {
    Err(ConfigWriteError::Io)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn file_identity_from_file(_file: &File) -> Result<FileIdentity, ConfigWriteError> {
    Err(ConfigWriteError::Io)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn path_matches_identity(_path: &Path, _expected: FileIdentity) -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub(super) fn read_regular_file(_path: &Path) -> Result<(Vec<u8>, FileIdentity), ConfigWriteError> {
    Err(ConfigWriteError::Io)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn copy_security_metadata_guarded(
    _source: &Path,
    _expected_source: FileIdentity,
    _destination: &Path,
    _expected_destination: FileIdentity,
) -> Result<(), ConfigWriteError> {
    Err(ConfigWriteError::Io)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn replace_existing(
    _target: &Path,
    _replacement: &Path,
    _backup: &Path,
    _expected_target: FileIdentity,
    _expected_replacement: FileIdentity,
) -> Result<(), ConfigWriteError> {
    Err(ConfigWriteError::Io)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn remove_if_identity(
    _path: &Path,
    _expected: FileIdentity,
) -> Result<(), ConfigWriteError> {
    Err(ConfigWriteError::Io)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn commit_new(_replacement: &Path, _target: &Path) -> Result<(), ConfigWriteError> {
    Err(ConfigWriteError::Io)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn restore_backup_if_matches(
    _target: &Path,
    _backup: &Path,
    _expected: &[u8],
    _quarantine: &Path,
) -> Result<(), ConfigWriteError> {
    Err(ConfigWriteError::RollbackFailed)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn guarded_remove(
    _target: &Path,
    _expected: &[u8],
    _quarantine: &Path,
) -> Result<(), ConfigWriteError> {
    Err(ConfigWriteError::RollbackFailed)
}
