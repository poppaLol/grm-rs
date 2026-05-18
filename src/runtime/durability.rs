use std::fs;
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::fsutil::{log_path, sync_parent_dir};
use crate::{RuntimeNodeModel, RuntimeRelModel, StoredNode, StoredRel};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DurableOperation {
    RegisterNodeModel { model: RuntimeNodeModel },
    RegisterRelModel { model: RuntimeRelModel },
    UpsertNode { node: StoredNode },
    DeleteNode { id: i64 },
    UpsertRel { rel: StoredRel },
    DeleteRel { id: i64 },
    Batch { ops: Vec<DurableOperation> },
}

pub(crate) fn append_operation(path: &Path, entry: &DurableOperation) -> io::Result<()> {
    let log_path = log_path(path);
    let log_existed = log_path.exists();
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    let line = serde_json::to_vec(entry).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "failed to serialize durable operation",
        )
    })?;
    file.write_all(&line)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    if !log_existed {
        sync_parent_dir(path)?;
    }
    Ok(())
}

pub(crate) fn clear_log(path: &Path) -> io::Result<()> {
    let log_path = log_path(path);
    match fs::remove_file(log_path) {
        Ok(()) => sync_parent_dir(path),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub(crate) fn read_operations(path: &Path) -> crate::Result<Vec<DurableOperation>> {
    let log_path = log_path(path);
    let bytes = match fs::read(&log_path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => {
            return Err(crate::error::GrmError::LoadAborted(
                "failed to read durable append log file",
            ));
        }
    };

    let mut entries = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        let Some(relative_end) = bytes[start..].iter().position(|byte| *byte == b'\n') else {
            // A final record without a newline may be a torn write. Ignore it:
            // earlier newline-terminated records remain committed replay input.
            break;
        };
        let end = start + relative_end;
        let line = &bytes[start..end];
        start = end + 1;
        if line.iter().all(|byte| byte.is_ascii_whitespace()) {
            continue;
        }
        let entry = serde_json::from_slice(line).map_err(|_| {
            crate::error::GrmError::LoadAborted("malformed durable append log record")
        })?;
        entries.push(entry);
    }

    Ok(entries)
}
