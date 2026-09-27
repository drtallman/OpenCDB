//! OpenCDB metadata binding for GeoPackage 1.2.1.
#[cfg(not(feature = "gpkg-metadata"))]
use super::MetadataEncoding;
use super::MetadataError;
use std::path::Path;
#[cfg(feature = "gpkg-metadata")]
mod io;
#[cfg(feature = "gpkg-metadata")]
mod schema;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecordKind {
    Global,
    Resource,
    Collection,
}
#[cfg(feature = "gpkg-metadata")]
impl RecordKind {
    pub(crate) fn schema_uri(self) -> &'static str {
        match self {
            Self::Global => {
                "https://github.com/drtallman/OpenCDB/blob/main/docs/GPKG_METADATA.md#global-v1"
            }
            Self::Resource => {
                "https://github.com/drtallman/OpenCDB/blob/main/docs/GPKG_METADATA.md#resource-v1"
            }
            Self::Collection => {
                "https://github.com/drtallman/OpenCDB/blob/main/docs/GPKG_METADATA.md#collection-v1"
            }
        }
    }
}
#[cfg(not(feature = "gpkg-metadata"))]
pub(crate) fn read_document(_path: &Path, _kind: RecordKind) -> Result<String, MetadataError> {
    Err(MetadataError::UnsupportedEncoding(MetadataEncoding::Gpkg))
}
#[cfg(all(test, feature = "gpkg-metadata"))]
mod tests;

#[cfg(feature = "gpkg-metadata")]
pub(crate) fn read_document(path: &Path, kind: RecordKind) -> Result<String, MetadataError> {
    let conn = io::open_readonly(path)?;
    schema::read_document(&conn, kind)
}
#[cfg(feature = "gpkg-metadata")]
fn malformed(reason: impl Into<String>) -> MetadataError {
    MetadataError::Serialization(reason.into())
}
#[cfg(feature = "gpkg-metadata")]
fn unsupported(reason: impl Into<String>) -> MetadataError {
    MetadataError::UnsupportedContainer {
        reason: reason.into(),
    }
}

pub(crate) struct PreparedWrite {
    #[cfg(feature = "gpkg-metadata")]
    target: std::path::PathBuf,
    #[cfg(feature = "gpkg-metadata")]
    bytes: Vec<u8>,
}

#[cfg(not(feature = "gpkg-metadata"))]
pub(crate) fn prepare_write(
    _target: &Path,
    _kind: RecordKind,
    _json: &str,
    _timestamp: chrono::DateTime<chrono::Utc>,
) -> Result<PreparedWrite, MetadataError> {
    Err(MetadataError::UnsupportedEncoding(MetadataEncoding::Gpkg))
}

#[cfg(feature = "gpkg-metadata")]
pub(crate) fn prepare_write(
    target: &Path,
    kind: RecordKind,
    json: &str,
    timestamp: chrono::DateTime<chrono::Utc>,
) -> Result<PreparedWrite, MetadataError> {
    match std::fs::symlink_metadata(target) {
        Ok(_) => {
            let existing = read_document(target, kind)?;
            let _: serde_json::Value = serde_json::from_str(&existing).map_err(super::ser_err)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let _: serde_json::Value = serde_json::from_str(json).map_err(super::ser_err)?;
    let mut connection = rusqlite::Connection::open_in_memory().map_err(io::sql_error)?;
    {
        let tx = connection.transaction().map_err(io::sql_error)?;
        tx.execute_batch(include_str!("schema.sql"))
            .map_err(io::sql_error)?;
        tx.execute("INSERT INTO gpkg_metadata (id,md_scope,md_standard_uri,mime_type,metadata) VALUES (1,'dataset',?1,'application/json',?2)",
            [kind.schema_uri(), json]).map_err(io::sql_error)?;
        tx.execute("INSERT INTO gpkg_metadata_reference (reference_scope,table_name,column_name,row_id_value,timestamp,md_file_id,md_parent_id) VALUES ('geopackage',NULL,NULL,NULL,?1,1,NULL)",
            [timestamp.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)]).map_err(io::sql_error)?;
        tx.commit().map_err(io::sql_error)?;
    }
    // Keep PreparedWrite an invariant: the entire container is valid before I/O.
    schema::read_document(&connection, kind)?;
    let bytes = connection
        .serialize("main")
        .map_err(io::sql_error)?
        .to_vec();
    connection
        .close()
        .map_err(|(_, error)| io::sql_error(error))?;
    Ok(PreparedWrite {
        target: target.to_owned(),
        bytes,
    })
}

impl PreparedWrite {
    #[cfg(feature = "gpkg-metadata")]
    pub(crate) fn install(self) -> Result<std::path::PathBuf, MetadataError> {
        io::install_with(self, |file, path| {
            file.persist(path).map(|_| ()).map_err(|error| error.error)
        })
    }
    #[cfg(not(feature = "gpkg-metadata"))]
    pub(crate) fn install(self) -> Result<std::path::PathBuf, MetadataError> {
        Err(MetadataError::UnsupportedEncoding(MetadataEncoding::Gpkg))
    }
}
