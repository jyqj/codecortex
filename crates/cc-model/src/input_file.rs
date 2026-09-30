//! Bounded regular-file reads. Unix walks descriptor-relative, never following
//! symlinks below the trusted project root. Other targets use a checked path
//! walk, not a claimed race-proof filesystem snapshot.
use std::{fs::File, io::Read, path::Path};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadError {
    Path,
    Root,
    Metadata,
    Symlink,
    NotRegular,
    Read,
    Size,
}
impl ReadError {
    pub fn config_reason(self) -> &'static str {
        match self {
            Self::Path => "unsupported_config_path",
            Self::Root => "project_root_unavailable",
            Self::Metadata => "config_metadata_failed",
            Self::Symlink => "symlink_config_unsupported",
            Self::NotRegular => "config_not_regular",
            Self::Read => "config_read_failed",
            Self::Size => "config_size_limit",
        }
    }
}
pub fn read(root: &Path, path: &str, limit: usize) -> Result<Option<Vec<u8>>, ReadError> {
    if !crate::repo_path::is_canonical_file(path) || path.split('/').count() > 256 {
        return Err(ReadError::Path);
    }
    let Some(file) = open(root, path)? else {
        return Ok(None);
    };
    let metadata = file.metadata().map_err(|_| ReadError::Metadata)?;
    if !metadata.is_file() {
        return Err(ReadError::NotRegular);
    }
    if metadata.len() > limit as u64 {
        return Err(ReadError::Size);
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ReadError::Read)?;
    if bytes.len() > limit {
        return Err(ReadError::Size);
    }
    Ok(Some(bytes))
}
#[cfg(unix)]
fn open(root: &Path, path: &str) -> Result<Option<File>, ReadError> {
    use std::os::unix::fs::OpenOptionsExt;
    let anchor = root.canonicalize().map_err(|_| ReadError::Root)?;
    let dir = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(anchor)
        .map_err(|_| ReadError::Root)?;
    open_from(dir, path)
}
#[cfg(unix)]
fn open_from(mut dir: File, path: &str) -> Result<Option<File>, ReadError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let mut parts = path.split('/').peekable();
    while let Some(part) = parts.next() {
        let name = std::ffi::CString::new(part).map_err(|_| ReadError::Path)?;
        let last = parts.peek().is_none();
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: dir owns a live descriptor, name is NUL-terminated, stat is
        // writable storage. It is assumed initialized only on successful fstatat.
        let rc = unsafe {
            libc::fstatat(
                dir.as_raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if rc != 0 {
            let e = std::io::Error::last_os_error();
            return if e.kind() == std::io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(ReadError::Metadata)
            };
        }
        // SAFETY: successful fstatat initialized the complete stat record.
        let mode = unsafe { stat.assume_init() }.st_mode & libc::S_IFMT;
        if mode == libc::S_IFLNK {
            return Err(ReadError::Symlink);
        }
        if (last && mode != libc::S_IFREG) || (!last && mode != libc::S_IFDIR) {
            return Err(ReadError::NotRegular);
        }
        // NOFOLLOW closes the check/open symlink gap; NONBLOCK prevents a FIFO
        // swapped into the final component from stalling before metadata checks.
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if last { 0 } else { libc::O_DIRECTORY };
        // SAFETY: live parent descriptor and NUL-terminated component; no creation
        // flags, so openat requires no variadic mode argument.
        let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            let e = std::io::Error::last_os_error();
            return if e.kind() == std::io::ErrorKind::NotFound {
                Ok(None)
            } else if e.raw_os_error() == Some(libc::ELOOP) {
                Err(ReadError::Symlink)
            } else {
                Err(ReadError::Read)
            };
        }
        // SAFETY: successful openat returned a new owned descriptor, consumed once.
        dir = unsafe { File::from_raw_fd(fd) };
        let m = dir.metadata().map_err(|_| ReadError::Metadata)?;
        if (last && !m.is_file()) || (!last && !m.is_dir()) {
            return Err(ReadError::NotRegular);
        }
    }
    Ok(Some(dir))
}
#[cfg(not(unix))]
fn open(root: &Path, path: &str) -> Result<Option<File>, ReadError> {
    let mut full = root.canonicalize().map_err(|_| ReadError::Root)?;
    let mut parts = path.split('/').peekable();
    while let Some(part) = parts.next() {
        full.push(part);
        let m = match full.symlink_metadata() {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ReadError::Metadata),
        };
        if m.file_type().is_symlink() {
            return Err(ReadError::Symlink);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                return Err(ReadError::Symlink);
            }
        }
        if (parts.peek().is_none() && !m.is_file()) || (parts.peek().is_some() && !m.is_dir()) {
            return Err(ReadError::NotRegular);
        }
    }
    File::open(full).map(Some).map_err(|_| ReadError::Read)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bound_and_path_failures_do_not_read_arbitrary_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("small"), "abcd").unwrap();
        assert_eq!(read(root.path(), "small", 4), Ok(Some(b"abcd".to_vec())));
        assert_eq!(read(root.path(), "small", 3), Err(ReadError::Size));
        assert_eq!(read(root.path(), "missing", 3), Ok(None));
        for path in ["../small", "/small", "a//small", "a/../small"] {
            assert_eq!(read(root.path(), path, 4), Err(ReadError::Path));
        }
    }
    #[cfg(unix)]
    #[test]
    fn descriptor_anchor_survives_path_replacement_without_following_new_symlink() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("root");
        let outside = base.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(root.join("input"), "safe").unwrap();
        std::fs::write(outside.join("input"), "wrong").unwrap();
        let anchor = File::open(&root).unwrap();
        std::fs::rename(&root, base.path().join("held")).unwrap();
        std::os::unix::fs::symlink(&outside, &root).unwrap();
        let mut file = open_from(anchor, "input").unwrap().unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        assert_eq!(text, "safe");
    }
}
