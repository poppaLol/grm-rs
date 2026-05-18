use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn write_file_atomically(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    let path = path.as_ref();
    let temp_path = atomic_temp_path(path);

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);

    fs::rename(&temp_path, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp_path);
    })?;
    sync_parent_dir(path)
}

pub(crate) fn write_file_atomically_with_backup(
    path: impl AsRef<Path>,
    bytes: &[u8],
) -> io::Result<()> {
    let path = path.as_ref();
    write_file_atomically(path, bytes)?;
    let backup = backup_path(path);
    write_file_atomically(&backup, bytes)?;
    Ok(())
}

pub(crate) fn sync_parent_dir(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    OpenOptions::new().read(true).open(parent)?.sync_all()
}

pub(crate) fn backup_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("grm-data");

    parent.join(format!("{file_name}.bak"))
}

pub(crate) fn log_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("grm-data");

    parent.join(format!("{file_name}.log"))
}

fn atomic_temp_path(path: &Path) -> PathBuf {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("grm-temp");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    parent.join(format!(".{file_name}.tmp-{}-{nonce}", std::process::id()))
}
