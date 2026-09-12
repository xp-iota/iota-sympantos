//! Shared filesystem security primitives: owner-only directories and atomic,
//! permission-locked file writes.
//!
//! These helpers back the persistence hardening required across the daemon,
//! config loader, kanban store, and skill cache: sensitive local files (auth
//! tokens, config with API keys, SQLite databases) must never be readable by
//! other local users, and writes must not leave a corrupt/partial file
//! behind if the process is interrupted mid-write.
//!
//! # Platform behaviour
//! - Unix: directories are created/chmod'd to `0700`, files to `0600`.
//! - Windows: files and directories receive a protected owner-only DACL;
//!   secure temp files are created with that descriptor before becoming
//!   visible, and replacement uses `MoveFileExW` with write-through.

use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(windows)]
mod windows_acl {
    use std::{
        ffi::c_void,
        fs::File,
        os::windows::{ffi::OsStrExt, io::FromRawHandle},
        path::Path,
        ptr::null_mut,
    };

    type Handle = *mut c_void;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const CREATE_NEW: u32 = 1;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;
    const SDDL_REVISION_1: u32 = 1;
    const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;

    #[repr(C)]
    struct SecurityAttributes {
        length: u32,
        security_descriptor: *mut c_void,
        inherit_handle: i32,
    }

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
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            attributes: *mut SecurityAttributes,
            disposition: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
        fn MoveFileExW(source: *const u16, destination: *const u16, flags: u32) -> i32;
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    fn descriptor() -> Result<*mut c_void, String> {
        // Protected DACL with one full-access ACE for Owner Rights.
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
        Ok(descriptor)
    }

    pub fn apply(path: &Path) -> Result<(), String> {
        let descriptor = descriptor()?;
        let result =
            unsafe { SetFileSecurityW(wide(path).as_ptr(), DACL_SECURITY_INFORMATION, descriptor) };
        unsafe { LocalFree(descriptor) };
        if result == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }

    pub fn create_new(path: &Path) -> Result<File, String> {
        let descriptor = descriptor()?;
        let mut attributes = SecurityAttributes {
            length: std::mem::size_of::<SecurityAttributes>() as u32,
            security_descriptor: descriptor,
            inherit_handle: 0,
        };
        let handle = unsafe {
            CreateFileW(
                wide(path).as_ptr(),
                GENERIC_WRITE,
                0,
                &mut attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        unsafe { LocalFree(descriptor) };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(unsafe { File::from_raw_handle(handle) })
    }

    pub fn replace(source: &Path, destination: &Path) -> Result<(), String> {
        if unsafe {
            MoveFileExW(
                wide(source).as_ptr(),
                wide(destination).as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
}

/// Ensures `dir` exists and is restricted to the owner only.
///
/// Unix uses mode `0700`. Windows applies a protected DACL containing only
/// an Owner Rights full-access ACE, preventing inherited broad ACLs.
pub fn ensure_dir_owner_only(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("failed to create directory {:?}", dir))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("failed to set 0700 permissions on {:?}", dir))?;
    }
    #[cfg(windows)]
    windows_acl::apply(dir)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("failed to apply owner-only DACL to {:?}", dir))?;

    Ok(())
}

/// Creates a missing directory with owner-only permissions without changing
/// permissions on an existing caller-owned directory.
///
/// Use this for parent directories derived from an arbitrary file path. The
/// caller still locks down the sensitive file itself, but must not chmod a
/// shared directory such as the system temporary directory.
pub fn create_missing_dir_owner_only(dir: &Path) -> Result<()> {
    if dir.exists() {
        return Ok(());
    }
    ensure_dir_owner_only(dir)
}

/// Sets file permissions to owner-only (`0600` on Unix, a protected
/// owner-only DACL on Windows).
pub fn set_file_owner_only(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to set 0600 permissions on {:?}", path))?;
    }
    #[cfg(windows)]
    windows_acl::apply(path)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("failed to apply owner-only DACL to {:?}", path))?;
    Ok(())
}

fn create_temp_owner_only(path: &Path) -> Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("failed to create owner-only temp file {:?}", path))
    }
    #[cfg(windows)]
    {
        return windows_acl::create_new(path)
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("failed to create owner-only temp file {:?}", path));
    }
    #[cfg(not(any(unix, windows)))]
    {
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .with_context(|| format!("failed to create temp file {:?}", path))
    }
}

/// Atomically writes `contents` to `path` with owner-only permissions.
///
/// Sequence: write to a sibling temp file → `sync_all` (flush to disk) →
/// lock down permissions to `0600` → `rename` over the destination (atomic
/// on the same filesystem) → best-effort `fsync` the parent directory so the
/// rename itself is durable across a crash.
///
/// This guarantees readers never observe a partially written file: they see
/// either the previous complete contents or the new complete contents, never
/// a truncated/interleaved write. The temp file uses a random suffix so
/// concurrent writers to the same path do not collide.
pub fn atomic_write_secure(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    create_missing_dir_owner_only(parent)?;

    let tmp_path = temp_sibling_path(path);

    {
        let mut file = create_temp_owner_only(&tmp_path)?;
        file.write_all(contents)
            .with_context(|| format!("failed to write temp file {:?}", tmp_path))?;
        file.sync_all()
            .with_context(|| format!("failed to fsync temp file {:?}", tmp_path))?;
    }

    set_file_owner_only(&tmp_path).with_context(|| {
        format!(
            "failed to lock down permissions on temp file {:?}",
            tmp_path
        )
    })?;

    #[cfg(windows)]
    windows_acl::replace(&tmp_path, path)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("failed to atomically replace {:?}", path))?;
    #[cfg(not(windows))]
    fs::rename(&tmp_path, path)
        .with_context(|| format!("failed to atomically rename {:?} to {:?}", tmp_path, path))?;

    fsync_dir_best_effort(parent);

    Ok(())
}

/// Builds a temp-file path alongside `path`, using a random suffix so
/// concurrent writers/retries cannot collide and readers of the destination
/// are never exposed to a half-written file.
fn temp_sibling_path(path: &Path) -> PathBuf {
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("tmp");
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let suffix = random_suffix();
    parent.join(format!(".{}.{}.tmp", file_name, suffix))
}

/// Generates a short random hex suffix using the process' CSPRNG source, to
/// avoid predictable temp file names (which could otherwise be raced by a
/// local attacker via symlink pre-creation).
fn random_suffix() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 8];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Best-effort directory fsync so a completed rename is durable across a
/// crash immediately following it. Failures are intentionally swallowed:
/// this is a durability nice-to-have, not required for correctness of the
/// atomic rename itself, and some filesystems/platforms do not support
/// opening+fsyncing a directory.
fn fsync_dir_best_effort(dir: &Path) {
    #[cfg(unix)]
    {
        if let Ok(dir_file) = File::open(dir) {
            let _ = dir_file.sync_all();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
}

/// Generates a cryptographically random token of `len` bytes using the OS
/// CSPRNG, returned as a lowercase hex string (`len * 2` characters).
///
/// Used for daemon auth tokens and similar local-secret material. Must never
/// be derived from timestamps, PIDs, or other low-entropy/guessable sources.
pub fn generate_csprng_token_hex(len: usize) -> String {
    use rand::RngCore;
    let mut bytes = vec![0u8; len];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Constant-time comparison of two byte slices, safe for comparing secrets
/// (tokens, MACs) without leaking timing information about where the first
/// mismatching byte is. Returns `false` immediately (still in constant time
/// relative to the shorter/likely-mismatched-length input) if lengths
/// differ, since unequal-length secrets can never be equal.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;

    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

#[cfg(test)]
#[path = "fs_secure_tests.rs"]
mod tests;
