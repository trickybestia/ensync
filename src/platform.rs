// Platform-specific details of the local filesystem and command shell.

use std::ffi::{OsStr, OsString};
use std::fs::{self, DirBuilder, Metadata};
use std::io;
use std::path::Path;
use std::process::Command;

#[cfg(unix)]
use std::os::unix::{
    ffi::OsStringExt,
    fs::{DirBuilderExt, MetadataExt, PermissionsExt},
};
#[cfg(windows)]
use std::os::windows::{
    fs::{MetadataExt, OpenOptionsExt},
    process::CommandExt,
};

use crate::defs::{FileMode, FileTime};
use crate::errors::{ErrorKind, Result};

pub fn decode_os_string(bytes: Vec<u8>) -> io::Result<OsString> {
    if bytes.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "NUL in path"));
    }
    #[cfg(unix)]
    return Ok(OsString::from_vec(bytes));
    #[cfg(windows)]
    return String::from_utf8(bytes).map(OsString::from).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "path is not valid UTF-8 on Windows",
        )
    });
}

pub fn check_encoding(name: &OsStr) -> Result<()> {
    if cfg!(windows) && name.to_str().is_none() {
        return Err(ErrorKind::BadFilename(name.to_owned()).into());
    }
    Ok(())
}

/// A single literal filesystem name, never a path or a device name.
pub fn check_filename(name: &OsStr) -> Result<()> {
    let bytes = name.as_encoded_bytes();
    if bytes.is_empty()
        || matches!(bytes, b"." | b"..")
        || bytes.iter().any(|&b| b == 0 || b == b'/')
    {
        return Err(ErrorKind::BadFilename(name.to_owned()).into());
    }
    if cfg!(windows) {
        check_encoding(name)?;
        let name_str = name.to_str().unwrap();
        let stem = name_str
            .split('.')
            .next()
            .unwrap()
            .trim_end_matches(' ')
            .to_ascii_uppercase();
        let device = matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|n| {
                matches!(
                    n,
                    "0" | "1"
                        | "2"
                        | "3"
                        | "4"
                        | "5"
                        | "6"
                        | "7"
                        | "8"
                        | "9"
                        | "¹"
                        | "²"
                        | "³"
                )
            });
        if device
            || name_str.ends_with([' ', '.'])
            || bytes.iter().any(|&b| b < 32 || b"\\:<>\"|?*".contains(&b))
        {
            return Err(ErrorKind::BadFilename(name.to_owned()).into());
        }
    }
    Ok(())
}

/// Windows can resolve a differently cased name or an 8.3 alias to an
/// existing file. Never mutate such a file as if it had the requested name.
pub fn check_exact_name(path: &Path) -> Result<()> {
    #[cfg(windows)]
    if let Some(name) = path.file_name() {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        for entry in fs::read_dir(parent)? {
            if entry?.file_name() == name {
                return Ok(());
            }
        }
        return Err(ErrorKind::ExpectationNotMatched.into());
    }
    let _ = path;
    Ok(())
}

pub fn is_reparse_point(md: &Metadata) -> bool {
    #[cfg(windows)]
    return md.file_attributes() & 0x400 != 0; // FILE_ATTRIBUTE_REPARSE_POINT
    #[cfg(unix)]
    {
        let _ = md;
        false
    }
}

pub fn file_mode(md: &Metadata) -> FileMode {
    #[cfg(unix)]
    return md.mode() & 0o7777;
    #[cfg(windows)]
    return if md.is_dir() { 0o755 } else { 0o644 };
}

pub fn dir_builder(mode: FileMode) -> DirBuilder {
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    builder.mode(mode);
    #[cfg(windows)]
    let _ = (&mut builder, mode);
    builder
}

pub fn set_mode(path: &Path, mode: FileMode) -> io::Result<()> {
    #[cfg(unix)]
    return fs::set_permissions(path, fs::Permissions::from_mode(mode));
    #[cfg(windows)]
    {
        let _ = (path, mode);
        Ok(())
    }
}

pub fn mtime(md: &Metadata) -> FileTime {
    filetime::FileTime::from_last_modification_time(md).unix_seconds()
}

/// Higher precision on Windows, where the time is available without a syscall.
pub fn cache_mtime(md: &Metadata) -> i64 {
    #[cfg(unix)]
    return mtime(md);
    #[cfg(windows)]
    return md.last_write_time() as i64;
}

pub fn file_id(path: &Path) -> io::Result<(u64, u64)> {
    #[cfg(unix)]
    {
        let md = fs::symlink_metadata(path)?;
        Ok((md.dev(), md.ino()))
    }
    #[cfg(windows)]
    {
        const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const BACKUP_SEMANTICS: u32 = 0x0200_0000;
        let file = fs::OpenOptions::new()
            .access_mode(0) // Query metadata without requesting read or write access.
            .custom_flags(BACKUP_SEMANTICS | OPEN_REPARSE_POINT)
            .open(path)?;
        let id = winapi_util::file::information(&file)?;
        Ok((id.volume_serial_number(), id.file_index()))
    }
}

pub fn symlink(target: impl AsRef<Path>, path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, path);
    #[cfg(windows)]
    {
        let _ = (target, path);
        // The wire format does not record whether the target is a file or
        // directory. Windows needs this information even for dangling links.
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "creating symbolic links is not supported on Windows",
        ))
    }
}

pub fn shell_command(command: &str) -> Command {
    #[cfg(unix)]
    {
        let mut process = Command::new("/bin/sh");
        process.arg("-c").arg(command);
        process
    }
    #[cfg(windows)]
    {
        // Resolve the system shell from an absolute path, not the workdir/PATH.
        let shell = std::env::var_os("COMSPEC")
            .filter(|p| Path::new(p).is_absolute())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(r"C:\Windows\System32\cmd.exe")
            });
        let mut process = Command::new(shell);
        // cmd.exe parses the command tail itself, not as CRT-style arguments.
        process
            .args(["/D", "/S", "/C"])
            .raw_arg(format!("\"{command}\""));
        process
    }
}
