//! Copy local files and directories into Dropbox.

use dropbox_sdk::default_client::UserAuthDefaultClient;
use dropbox_sdk::files::{
    self, CreateFolderArg, CreateFolderError, UploadArg, UploadSessionAppendArg,
    UploadSessionCursor, UploadSessionFinishArg, UploadSessionStartArg, UploadSessionStartResult,
    WriteConflictError, WriteError, WriteMode,
};
use dropbox_sdk::Error;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;
use walkdir::WalkDir;

/// Dropbox single-request upload limit (see `files/upload` docs).
const MAX_CHUNK: usize = 150 * 1024 * 1024;

/// Normalize `DEST` to a Dropbox path starting with `/`.
pub fn normalize_dropbox_path(dest: &str) -> String {
    let t = dest.trim();
    if t.is_empty() {
        return "/".to_string();
    }
    if t.starts_with('/') {
        t.to_string()
    } else {
        format!("/{}", t.trim_start_matches('/'))
    }
}

/// Join a Dropbox folder prefix with a relative path using `/` separators.
pub fn join_dropbox_path(base: &str, rel: &Path) -> String {
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    let base_trim = base.trim_end_matches('/');
    if rel_str.is_empty() || rel_str == "." {
        base_trim.to_string()
    } else {
        format!("{base_trim}/{rel_str}")
    }
}

fn write_mode(force: bool) -> WriteMode {
    if force {
        WriteMode::Overwrite
    } else {
        WriteMode::Add
    }
}

fn is_conflict(err: &WriteError) -> bool {
    matches!(err, WriteError::Conflict(_))
}

fn upload_conflict(e: &Error<files::UploadError>) -> bool {
    match e {
        Error::Api(files::UploadError::Path(f)) => is_conflict(&f.reason),
        _ => false,
    }
}

fn finish_conflict(e: &Error<files::UploadSessionFinishError>) -> bool {
    match e {
        Error::Api(files::UploadSessionFinishError::Path(we)) => is_conflict(we),
        _ => false,
    }
}

fn retry_rate_limit<T, E>(
    mut op: impl FnMut() -> Result<T, Error<E>>,
) -> Result<T, Error<E>> {
    let mut last_rate_limit = None;
    for attempt in 0..6 {
        match op() {
            Ok(v) => return Ok(v),
            Err(
                e @ Error::RateLimited {
                    retry_after_seconds,
                    ..
                },
            ) => {
                last_rate_limit = Some(e);
                if attempt == 5 {
                    break;
                }
                sleep(Duration::from_secs(u64::from(retry_after_seconds.max(1))));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_rate_limit.expect("retry loop"))
}

fn create_dropbox_folder(client: &UserAuthDefaultClient, path: &str, label: &str) -> bool {
    match files::create_folder_v2(client, &CreateFolderArg::new(path.to_string())) {
        Ok(_) => false,
        Err(Error::Api(CreateFolderError::Path(ref we))) if mkdir_ok(we) => false,
        Err(e) => {
            eprintln!("dcp: could not create {label} {path:?}: {e}");
            true
        }
    }
}

fn mkdir_ok(err: &WriteError) -> bool {
    matches!(err, WriteError::Conflict(WriteConflictError::Folder))
}

fn upload_small(
    client: &UserAuthDefaultClient,
    bytes: &[u8],
    dropbox_path: &str,
    mode: WriteMode,
) -> Result<(), Error<files::UploadError>> {
    let arg = UploadArg::new(dropbox_path.to_string()).with_mode(mode);
    retry_rate_limit(|| files::upload(client, &arg, bytes)).map(|_| ())
}

fn upload_large(
    client: &UserAuthDefaultClient,
    file: &mut File,
    len: u64,
    dropbox_path: &str,
    mode: WriteMode,
) -> Result<(), Error<files::UploadSessionFinishError>> {
    let mut buf = vec![0u8; MAX_CHUNK.min(len as usize).max(1)];
    let n = read_at_least(file, &mut buf[..(len as usize).min(MAX_CHUNK)])
        .map_err(|e| Error::UnexpectedResponse(e.to_string()))?;
    let start = with_start_retries(client, &UploadSessionStartArg::default(), &buf[..n])
        .map_err(|e| Error::UnexpectedResponse(e.to_string()))?;
    let mut uploaded = n as u64;
    let session_id = start.session_id;

    while len - uploaded > MAX_CHUNK as u64 {
        let n = read_at_least(file, &mut buf[..MAX_CHUNK])
            .map_err(|e| Error::UnexpectedResponse(e.to_string()))?;
        let cursor = UploadSessionCursor::new(session_id.clone(), uploaded);
        let arg = UploadSessionAppendArg::new(cursor);
        with_append_retries(client, &arg, &buf[..n])
            .map_err(|e| Error::UnexpectedResponse(e.to_string()))?;
        uploaded += n as u64;
    }

    let remaining = (len - uploaded) as usize;
    if remaining > 0 {
        let n = read_at_least(file, &mut buf[..remaining])
            .map_err(|e| Error::UnexpectedResponse(e.to_string()))?;
        let cursor = UploadSessionCursor::new(session_id.clone(), uploaded);
        let arg = UploadSessionAppendArg::new(cursor).with_close(true);
        with_append_retries(client, &arg, &buf[..n])
            .map_err(|e| Error::UnexpectedResponse(e.to_string()))?;
        uploaded += n as u64;
    }
    debug_assert_eq!(uploaded, len);

    let commit = files::CommitInfo::new(dropbox_path.to_string()).with_mode(mode);
    let finish = UploadSessionFinishArg::new(
        UploadSessionCursor::new(session_id, len),
        commit,
    );
    with_finish_retries(client, &finish, &[]).map(|_| ())
}

fn read_at_least(f: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut got = 0;
    while got < buf.len() {
        let n = f.read(&mut buf[got..])?;
        if n == 0 {
            break;
        }
        got += n;
    }
    if got != buf.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "short read",
        ));
    }
    Ok(got)
}

fn with_append_retries(
    client: &UserAuthDefaultClient,
    arg: &UploadSessionAppendArg,
    body: &[u8],
) -> Result<(), Error<files::UploadSessionAppendError>> {
    retry_rate_limit(|| files::upload_session_append_v2(client, arg, body))
}

fn with_start_retries(
    client: &UserAuthDefaultClient,
    arg: &UploadSessionStartArg,
    body: &[u8],
) -> Result<UploadSessionStartResult, Error<files::UploadSessionStartError>> {
    retry_rate_limit(|| files::upload_session_start(client, arg, body))
}

fn with_finish_retries(
    client: &UserAuthDefaultClient,
    arg: &UploadSessionFinishArg,
    body: &[u8],
) -> Result<files::FileMetadata, Error<files::UploadSessionFinishError>> {
    retry_rate_limit(|| files::upload_session_finish(client, arg, body))
}

fn upload_one_file(
    client: &UserAuthDefaultClient,
    local: &Path,
    dropbox_path: &str,
    force: bool,
) -> bool {
    let mode = write_mode(force);
    let meta = match fs::metadata(local) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("dcp: skip {:?}: {e}", local);
            return true;
        }
    };
    let len = meta.len();
    if len > MAX_CHUNK as u64 {
        let mut file = match File::open(local) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("dcp: skip {:?}: {e}", local);
                return true;
            }
        };
        match upload_large(client, &mut file, len, dropbox_path, mode) {
            Ok(_) => false,
            Err(e) if finish_conflict(&e) => {
                eprintln!(
                    "dcp: file already exists at {dropbox_path:?}, skipping (use -f to overwrite)"
                );
                true
            }
            Err(e) => {
                eprintln!("dcp: upload failed for {:?} -> {dropbox_path:?}: {e}", local);
                true
            }
        }
    } else {
        let bytes = match fs::read(local) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("dcp: skip {:?}: {e}", local);
                return true;
            }
        };
        match upload_small(client, &bytes, dropbox_path, mode) {
            Ok(_) => false,
            Err(e) if upload_conflict(&e) => {
                eprintln!(
                    "dcp: file already exists at {dropbox_path:?}, skipping (use -f to overwrite)"
                );
                true
            }
            Err(e) => {
                eprintln!("dcp: upload failed for {:?} -> {dropbox_path:?}: {e}", local);
                true
            }
        }
    }
}

/// Copy `source` into Dropbox at `dest` (normalized). Returns `true` if any error occurred.
pub fn copy_to_dropbox(
    client: &UserAuthDefaultClient,
    source: &Path,
    dest: &str,
    recursive: bool,
    force: bool,
) -> bool {
    let dest = normalize_dropbox_path(dest);
    let meta = match fs::metadata(source) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("dcp: cannot read source {:?}: {e}", source);
            return true;
        }
    };

    if meta.is_dir() {
        if !recursive {
            eprintln!(
                "dcp: source {:?} is a directory; use -r or --recursive to copy it",
                source
            );
            return true;
        }
        return copy_dir(client, source, &dest, force);
    }

    if !meta.is_file() {
        eprintln!("dcp: source {:?} is not a file or directory", source);
        return true;
    }

    upload_one_file(client, source, &dest, force)
}

fn copy_dir(client: &UserAuthDefaultClient, source: &Path, dest: &str, force: bool) -> bool {
    let base = match fs::canonicalize(source) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("dcp: cannot canonicalize source {:?}: {e}", source);
            return true;
        }
    };

    let mut had_error = false;
    let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let mut file_count = 0usize;

    for entry in WalkDir::new(&base).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        let depth = entry.depth();
        if depth == 0 {
            continue;
        }

        let rel = match path.strip_prefix(&base) {
            Ok(r) => r.to_path_buf(),
            Err(_) => continue,
        };

        let meta = match fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("dcp: skip {:?}: {e}", path);
                had_error = true;
                continue;
            }
        };

        if meta.is_dir() {
            dirs.insert(rel);
            continue;
        }

        if !meta.is_file() {
            continue;
        }

        file_count += 1;
        let dropbox_path = join_dropbox_path(dest, &rel);
        if upload_one_file(client, path, &dropbox_path, force) {
            had_error = true;
        }
    }

    let mut dir_list: Vec<PathBuf> = dirs.into_iter().collect();
    dir_list.sort_by_key(|p| p.components().count());
    let no_subdirs = dir_list.is_empty();

    for rel in &dir_list {
        let dropbox_path = join_dropbox_path(dest, rel);
        if create_dropbox_folder(client, &dropbox_path, "folder") {
            had_error = true;
        }
    }

    if file_count == 0
        && no_subdirs
        && create_dropbox_folder(client, dest, "destination folder")
    {
        had_error = true;
    }

    had_error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_prepends_slash() {
        assert_eq!(normalize_dropbox_path("backup/stuff"), "/backup/stuff");
        assert_eq!(normalize_dropbox_path("/a"), "/a");
        assert_eq!(normalize_dropbox_path("  x  "), "/x");
    }

    #[test]
    fn join_paths() {
        assert_eq!(
            join_dropbox_path("/backup/stuff", Path::new("a/b.txt")),
            "/backup/stuff/a/b.txt"
        );
    }
}
