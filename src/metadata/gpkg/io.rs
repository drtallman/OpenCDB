//! File opening and SQLite error translation; never expose SQLite in the API.
use super::{MetadataError, malformed, unsupported};
use rusqlite::{Connection, ErrorCode, OpenFlags};
use std::{fs::File, io::Read, path::Path, time::Duration};

pub(super) fn sql_error(error: rusqlite::Error) -> MetadataError {
    match &error {
        rusqlite::Error::SqliteFailure(code, _)
            if matches!(
                code.code,
                ErrorCode::PermissionDenied
                    | ErrorCode::DatabaseBusy
                    | ErrorCode::DatabaseLocked
                    | ErrorCode::OutOfMemory
                    | ErrorCode::ReadOnly
                    | ErrorCode::OperationInterrupted
                    | ErrorCode::SystemIoFailure
                    | ErrorCode::DiskFull
                    | ErrorCode::CannotOpen
                    | ErrorCode::FileLockingProtocolFailed
            ) =>
        {
            MetadataError::Io(std::io::Error::other(error.to_string()))
        }
        _ => MetadataError::Serialization(error.to_string()),
    }
}

pub(super) fn open_readonly(path: &Path) -> Result<Connection, MetadataError> {
    let mut file = File::open(path)?;
    let mut header = [0; 100];
    if let Err(error) = file.read_exact(&mut header) {
        return if error.kind() == std::io::ErrorKind::UnexpectedEof {
            Err(malformed("truncated SQLite header"))
        } else {
            Err(error.into())
        };
    }
    if &header[..16] != b"SQLite format 3\0" {
        return Err(malformed("not a SQLite database"));
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut companion = path.as_os_str().to_owned();
        companion.push(suffix);
        match std::fs::symlink_metadata(&companion) {
            Ok(_) => return Err(unsupported(format!("pending {suffix} companion"))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    if header[18] != 1 || header[19] != 1 {
        return Err(unsupported(
            "only finalized rollback-journal databases are supported",
        ));
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(sql_error)?;
    conn.busy_timeout(Duration::ZERO).map_err(sql_error)?;
    conn.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON;")
        .map_err(sql_error)?;
    Ok(conn)
}

/// One-file publication. The persist seam permits deterministic failure tests.
pub(super) fn install_with(
    prepared: super::PreparedWrite,
    persist: impl FnOnce(tempfile::NamedTempFile, &Path) -> Result<(), std::io::Error>,
) -> Result<std::path::PathBuf, MetadataError> {
    let parent = prepared.target.parent().ok_or_else(|| {
        MetadataError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "metadata path has no parent",
        ))
    })?;
    let permissions = match std::fs::metadata(&prepared.target) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    std::fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    std::io::Write::write_all(&mut staged, &prepared.bytes)?;
    if let Some(permissions) = permissions {
        staged.as_file().set_permissions(permissions)?;
    }
    staged.as_file().sync_all()?;
    persist(staged, &prepared.target)?;
    Ok(prepared.target)
}
