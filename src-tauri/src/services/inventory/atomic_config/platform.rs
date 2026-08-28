use super::ConfigWriteError;
use std::path::Path;

#[cfg(target_os = "macos")]
pub(super) fn copy_security_metadata(
    source: &Path,
    destination: &Path,
) -> Result<(), ConfigWriteError> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_os_str().as_bytes()).map_err(|_| ConfigWriteError::Io)?;
    let destination =
        CString::new(destination.as_os_str().as_bytes()).map_err(|_| ConfigWriteError::Io)?;
    // SAFETY: Both C strings are NUL-terminated and remain alive for the call.
    let result = unsafe {
        libc::copyfile(
            source.as_ptr(),
            destination.as_ptr(),
            std::ptr::null_mut(),
            libc::COPYFILE_METADATA,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(ConfigWriteError::Io)
    }
}

#[cfg(target_os = "macos")]
pub(super) fn replace_existing(
    target: &Path,
    replacement: &Path,
    backup: &Path,
) -> Result<(), ConfigWriteError> {
    exchange_files(replacement, target)?;
    if std::fs::rename(replacement, backup).is_err() {
        return match exchange_files(replacement, target) {
            Ok(()) => Err(ConfigWriteError::Io),
            Err(_) => Err(ConfigWriteError::RollbackFailed),
        };
    }
    Ok(())
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
pub(super) fn copy_security_metadata(
    _source: &Path,
    _destination: &Path,
) -> Result<(), ConfigWriteError> {
    Err(ConfigWriteError::Io)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn replace_existing(
    _target: &Path,
    _replacement: &Path,
    _backup: &Path,
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
pub(super) fn restore_quarantine_without_overwrite(
    _quarantine: &Path,
    _target: &Path,
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
