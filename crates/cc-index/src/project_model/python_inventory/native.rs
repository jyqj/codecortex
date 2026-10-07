//! Linux descriptor-relative bounded enumeration. No unbounded list-all call.
use super::*;
use std::{
    collections::BTreeSet,
    ffi::{CStr, CString},
    fs::{File, Metadata, OpenOptions},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
};
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Stamp {
    dev: u64,
    ino: u64,
    pub(super) size: usize,
    mode: u32,
    links: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
}
fn stamp(m: Metadata) -> Result<Stamp> {
    Ok(Stamp {
        dev: m.dev(),
        ino: m.ino(),
        size: usize::try_from(m.len()).map_err(|_| CaptureRefusal::Budget("file_bytes"))?,
        mode: m.mode(),
        links: m.nlink(),
        mtime: (m.mtime(), m.mtime_nsec()),
        ctime: (m.ctime(), m.ctime_nsec()),
    })
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Inventory {
    mount: u64,
    pub files: BTreeMap<String, Stamp>,
    pub(super) dirs: BTreeMap<String, Stamp>,
}
fn open_root(root: &Path) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)
        .map_err(|_| CaptureRefusal::Drift)
}
fn mount_id(file: &File) -> Result<u64> {
    let mut value = std::mem::MaybeUninit::<libc::statx>::uninit();
    // SAFETY: live descriptor, static empty C string and writable statx storage.
    let rc = unsafe {
        libc::statx(
            file.as_raw_fd(),
            c"".as_ptr(),
            libc::AT_EMPTY_PATH,
            libc::STATX_MNT_ID,
            value.as_mut_ptr(),
        )
    };
    if rc != 0 {
        return Err(CaptureRefusal::UnsupportedPlatform);
    }
    // SAFETY: successful statx initialized the complete record.
    let value = unsafe { value.assume_init() };
    if value.stx_mask & libc::STATX_MNT_ID == 0 {
        return Err(CaptureRefusal::UnsupportedPlatform);
    }
    Ok(value.stx_mnt_id)
}
struct Directory(*mut libc::DIR);
impl Drop for Directory {
    fn drop(&mut self) {
        // SAFETY: exclusively owned successful fdopendir result.
        unsafe {
            libc::closedir(self.0);
        }
    }
}
fn directory(file: &File) -> Result<Directory> {
    // SAFETY: live descriptor; dup creates separate ownership for fdopendir.
    let fd = unsafe { libc::dup(file.as_raw_fd()) };
    if fd < 0 {
        return Err(CaptureRefusal::Io);
    }
    // SAFETY: fd is exclusively owned, transferred on success only.
    let dir = unsafe { libc::fdopendir(fd) };
    if dir.is_null() {
        // SAFETY: failed fdopendir did not consume fd.
        unsafe {
            libc::close(fd);
        }
        return Err(CaptureRefusal::Io);
    }
    Ok(Directory(dir))
}
fn portable(name: &str) -> bool {
    // Conservative portable spelling: ASCII only; no Windows reserved basename,
    // trailing dot/space, separators, alternate streams or control characters.
    let base = name.split('.').next().unwrap_or("").to_ascii_lowercase();
    !name.is_empty()
        && name.is_ascii()
        && !name.ends_with(['.', ' '])
        && !name
            .bytes()
            .any(|b| b < 32 || b == 127 || b"\\/:*?\"<>|".contains(&b))
        && !matches!(base.as_str(), "con" | "prn" | "aux" | "nul")
        && !(base.len() == 4
            && (base.starts_with("com") || base.starts_with("lpt"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
}
pub(super) fn inventory(root: &Path, limits: CaptureLimits) -> Result<Inventory> {
    let file = open_root(root)?;
    let mount = mount_id(&file)?;
    let mut result = Inventory {
        mount,
        files: BTreeMap::new(),
        dirs: BTreeMap::new(),
    };
    let mut state = State {
        mount,
        entries: 0,
        bytes: 0,
        paths: 0,
        aliases: BTreeSet::new(),
        ids: BTreeSet::new(),
        limits,
    };
    walk(file, "", 0, &mut state, &mut result)?;
    Ok(result)
}
struct State {
    mount: u64,
    entries: usize,
    bytes: usize,
    paths: usize,
    aliases: BTreeSet<String>,
    ids: BTreeSet<(u64, u64)>,
    limits: CaptureLimits,
}
fn walk(dir: File, path: &str, depth: usize, state: &mut State, out: &mut Inventory) -> Result<()> {
    if mount_id(&dir)? != state.mount {
        return Err(CaptureRefusal::Alias);
    }
    let before = stamp(dir.metadata().map_err(|_| CaptureRefusal::Io)?)?;
    if !state.ids.insert((before.dev, before.ino)) {
        return Err(CaptureRefusal::Alias);
    }
    state.entries = sum(state.entries, 1, "entries")?;
    bound(state.entries, state.limits.entries, "entries")?;
    out.dirs.insert(path.into(), before);
    let stream = directory(&dir)?;
    loop {
        // SAFETY: Linux thread-local errno and live, exclusively owned stream.
        unsafe {
            *libc::__errno_location() = 0;
        }
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            // SAFETY: thread-local errno following readdir.
            if unsafe { *libc::__errno_location() } != 0 {
                return Err(CaptureRefusal::Io);
            }
            break;
        }
        // SAFETY: readdir provides a NUL-terminated name valid until next call.
        let raw = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if raw.to_bytes() == b"." || raw.to_bytes() == b".." {
            continue;
        }
        // Count before allocating any name/key or opening the child.
        bound(
            sum(state.entries, 1, "entries")?,
            state.limits.entries,
            "entries",
        )?;
        bound(depth + 1, state.limits.depth.min(256), "depth")?;
        let name = raw.to_str().map_err(|_| CaptureRefusal::Path)?;
        if !portable(name) {
            return Err(CaptureRefusal::Path);
        }
        let length = sum(
            path.len(),
            name.len() + usize::from(!path.is_empty()),
            "path_bytes",
        )?;
        bound(length, state.limits.path_bytes.min(4096), "path_bytes")?;
        state.paths = sum(state.paths, length, "total_path_bytes")?;
        bound(
            state.paths,
            state.limits.total_path_bytes,
            "total_path_bytes",
        )?;
        let key = if path.is_empty() {
            name.into()
        } else {
            format!("{path}/{name}")
        };
        if !cc_model::repo_path::is_canonical_file(&key) {
            return Err(CaptureRefusal::Path);
        }
        if out.files.contains_key(&key) || out.dirs.contains_key(&key) {
            return Err(CaptureRefusal::DuplicateKey);
        }
        if !state.aliases.insert(key.to_ascii_lowercase()) {
            return Err(CaptureRefusal::Alias);
        }
        let name = CString::new(name).map_err(|_| CaptureRefusal::Path)?;
        let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: live parent, NUL-terminated name, writable stat storage.
        if unsafe {
            libc::fstatat(
                dir.as_raw_fd(),
                name.as_ptr(),
                st.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(CaptureRefusal::Drift);
        }
        // SAFETY: fstatat succeeded and initialized the record.
        let mode = unsafe { st.assume_init() }.st_mode & libc::S_IFMT;
        if mode != libc::S_IFDIR && mode != libc::S_IFREG {
            return Err(CaptureRefusal::SymlinkOrNonregular);
        }
        // NOFOLLOW refuses symlink swaps; NONBLOCK avoids FIFO stalls. Opened
        // metadata, rather than directory entry hints, decides the admitted kind.
        // SAFETY: live descriptor, NUL-terminated child, no creation flags.
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(CaptureRefusal::SymlinkOrNonregular);
        }
        // SAFETY: successful openat returns a fresh descriptor consumed once.
        let child = unsafe { File::from_raw_fd(fd) };
        if mount_id(&child)? != state.mount {
            return Err(CaptureRefusal::Alias);
        }
        let m = child.metadata().map_err(|_| CaptureRefusal::Io)?;
        if m.is_dir() {
            walk(child, &key, depth + 1, state, out)?;
        } else if m.is_file() {
            state.entries += 1;
            bound(out.files.len() + 1, state.limits.files, "files")?;
            let s = stamp(m)?;
            if s.links != 1 || !state.ids.insert((s.dev, s.ino)) {
                return Err(CaptureRefusal::Alias);
            }
            bound(s.size, state.limits.file_bytes, "file_bytes")?;
            state.bytes = sum(state.bytes, s.size, "total_bytes")?;
            bound(state.bytes, state.limits.total_bytes, "total_bytes")?;
            if out.files.insert(key, s).is_some() {
                return Err(CaptureRefusal::DuplicateKey);
            }
        } else {
            return Err(CaptureRefusal::SymlinkOrNonregular);
        }
    }
    if out.dirs[path] != stamp(dir.metadata().map_err(|_| CaptureRefusal::Io)?)? {
        return Err(CaptureRefusal::Drift);
    }
    Ok(())
}
pub(super) fn verify(
    anchor: &Path,
    requested: &Path,
    first: &Inventory,
    files: &BTreeMap<String, Vec<u8>>,
    limits: CaptureLimits,
) -> Result<()> {
    if requested
        .canonicalize()
        .map_err(|_| CaptureRefusal::Drift)?
        != anchor
    {
        return Err(CaptureRefusal::Drift);
    }
    if inventory(anchor, limits)? != *first {
        return Err(CaptureRefusal::Drift);
    }
    for (path, bytes) in files {
        if cc_model::input_file::read(anchor, path, limits.file_bytes)
            .map_err(|_| CaptureRefusal::Drift)?
            .as_ref()
            != Some(bytes)
        {
            return Err(CaptureRefusal::Drift);
        }
    }
    if inventory(anchor, limits)? != *first {
        return Err(CaptureRefusal::Drift);
    }
    Ok(())
}
