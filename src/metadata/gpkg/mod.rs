//! OpenCDB metadata binding for GeoPackage 1.2.1.
// Removed when the facade and journal callers land in tasks 3/5.
#![allow(dead_code)]
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
