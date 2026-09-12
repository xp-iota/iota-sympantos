use anyhow::{Context, Result};
use std::path::Path;

pub(super) fn prepare_sqlite_path(path: &Path) -> Result<()> {
    if path == Path::new(":memory:") {
        return Ok(());
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating kanban db dir {}", parent.display()))?;
        set_dir_owner_only(parent)?;
    }
    if !path.exists() {
        create_owner_only(path)?;
    } else {
        set_file_owner_only(path)?;
    }
    Ok(())
}

pub(super) fn secure_sqlite_files(path: &Path) -> Result<()> {
    if path == Path::new(":memory:") {
        return Ok(());
    }
    set_file_owner_only(path)?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    for suffix in ["-wal", "-shm"] {
        let sidecar = parent.join(format!("{name}{suffix}"));
        if sidecar.exists() {
            set_file_owner_only(&sidecar)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_dir_owner_only(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(unix)]
fn create_owner_only(path: &Path) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_file_owner_only(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(windows)]
mod windows {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::Path, ptr::null_mut};

    const SDDL_REVISION_1: u32 = 1;
    const DACL_SECURITY_INFORMATION: u32 = 4;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text: *const u16,
            revision: u32,
            descriptor: *mut *mut c_void,
            size: *mut u32,
        ) -> i32;
        fn SetFileSecurityW(path: *const u16, information: u32, descriptor: *const c_void) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    pub fn apply(path: &Path) -> Result<(), String> {
        let sddl: Vec<u16> = "D:P(A;;FA;;;OW)".encode_utf16().chain(Some(0)).collect();
        let mut descriptor = null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
            || descriptor.is_null()
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let result =
            unsafe { SetFileSecurityW(wide.as_ptr(), DACL_SECURITY_INFORMATION, descriptor) };
        unsafe { LocalFree(descriptor) };
        if result == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
}

#[cfg(windows)]
fn set_dir_owner_only(path: &Path) -> Result<()> {
    windows::apply(path).map_err(anyhow::Error::msg)
}

#[cfg(windows)]
fn create_owner_only(path: &Path) -> Result<()> {
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    windows::apply(path).map_err(anyhow::Error::msg)
}

#[cfg(windows)]
fn set_file_owner_only(path: &Path) -> Result<()> {
    windows::apply(path).map_err(anyhow::Error::msg)
}

#[cfg(not(any(unix, windows)))]
fn set_dir_owner_only(_path: &Path) -> Result<()> {
    anyhow::bail!("owner-only directory permissions are unsupported on this platform")
}
#[cfg(not(any(unix, windows)))]
fn create_owner_only(_path: &Path) -> Result<()> {
    anyhow::bail!("owner-only file permissions are unsupported on this platform")
}
#[cfg(not(any(unix, windows)))]
fn set_file_owner_only(_path: &Path) -> Result<()> {
    anyhow::bail!("owner-only file permissions are unsupported on this platform")
}
