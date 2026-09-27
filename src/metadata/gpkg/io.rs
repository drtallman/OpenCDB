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
