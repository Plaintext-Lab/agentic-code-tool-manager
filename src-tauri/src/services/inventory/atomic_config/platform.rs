use super::ConfigWriteError;
use std::path::Path;

#[cfg(unix)]
use std::fs;

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

#[cfg(target_os = "linux")]
pub(super) fn copy_security_metadata(
    source: &Path,
    destination: &Path,
) -> Result<(), ConfigWriteError> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::metadata(source).map_err(|_| ConfigWriteError::Io)?;
    let destination_file = fs::OpenOptions::new()
        .write(true)
        .open(destination)
        .map_err(|_| ConfigWriteError::Io)?;
    // SAFETY: The file descriptor is valid for this call and the ids came from stat.
    if unsafe { libc::fchown(destination_file.as_raw_fd(), metadata.uid(), metadata.gid()) } != 0 {
        return Err(ConfigWriteError::Io);
    }
    destination_file
        .set_permissions(metadata.permissions())
        .map_err(|_| ConfigWriteError::Io)?;
    for attribute in xattr::list(source).map_err(|_| ConfigWriteError::Io)? {
        let value = xattr::get(source, &attribute)
            .map_err(|_| ConfigWriteError::Io)?
            .ok_or(ConfigWriteError::Io)?;
        xattr::set(destination, &attribute, &value).map_err(|_| ConfigWriteError::Io)?;
    }
    Ok(())
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
pub(super) fn copy_security_metadata(
    _source: &Path,
    _destination: &Path,
) -> Result<(), ConfigWriteError> {
    Err(ConfigWriteError::Io)
}

#[cfg(windows)]
pub(super) fn copy_security_metadata(
    _source: &Path,
    _destination: &Path,
) -> Result<(), ConfigWriteError> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn replace_existing(
    target: &Path,
    replacement: &Path,
    backup: &Path,
) -> Result<(), ConfigWriteError> {
    exchange_files(replacement, target)?;
    if fs::rename(replacement, backup).is_err() {
        return match exchange_files(replacement, target) {
            Ok(()) => Err(ConfigWriteError::Io),
            Err(_) => Err(ConfigWriteError::RollbackFailed),
        };
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn restore_backup(target: &Path, backup: &Path) -> Result<(), ConfigWriteError> {
    exchange_files(backup, target).map_err(|_| ConfigWriteError::RollbackFailed)
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

#[cfg(target_os = "linux")]
fn exchange_files(left: &Path, right: &Path) -> Result<(), ConfigWriteError> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let left = CString::new(left.as_os_str().as_bytes()).map_err(|_| ConfigWriteError::Io)?;
    let right = CString::new(right.as_os_str().as_bytes()).map_err(|_| ConfigWriteError::Io)?;
    // SAFETY: Both C strings are NUL-terminated and remain alive for the call.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            left.as_ptr(),
            libc::AT_FDCWD,
            right.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(ConfigWriteError::Io)
    }
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn exchange_files(_left: &Path, _right: &Path) -> Result<(), ConfigWriteError> {
    Err(ConfigWriteError::Io)
}

#[cfg(windows)]
pub(super) fn replace_existing(
    target: &Path,
    replacement: &Path,
    backup: &Path,
) -> Result<(), ConfigWriteError> {
    replace_file(target, replacement, Some(backup)).map_err(|_| ConfigWriteError::Io)
}

#[cfg(windows)]
pub(super) fn restore_backup(target: &Path, backup: &Path) -> Result<(), ConfigWriteError> {
    replace_file(target, backup, None).map_err(|_| ConfigWriteError::RollbackFailed)
}

#[cfg(windows)]
fn replace_file(
    target: &Path,
    replacement: &Path,
    backup: Option<&Path>,
) -> Result<(), ConfigWriteError> {
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    let target: Vec<u16> = target.as_os_str().encode_wide().chain(once(0)).collect();
    let replacement: Vec<u16> = replacement
        .as_os_str()
        .encode_wide()
        .chain(once(0))
        .collect();
    let backup = backup.map(|path| {
        path.as_os_str()
            .encode_wide()
            .chain(once(0))
            .collect::<Vec<u16>>()
    });
    let backup_pointer = backup
        .as_ref()
        .map_or(std::ptr::null(), |path| path.as_ptr());
    // SAFETY: Every non-null pointer references a NUL-terminated buffer alive for the call.
    let result = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            replacement.as_ptr(),
            backup_pointer,
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if result == 0 {
        Err(ConfigWriteError::Io)
    } else {
        Ok(())
    }
}
