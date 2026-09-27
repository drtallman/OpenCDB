//! All GeoPackage metadata preparation precedes the collection's first mutation.
use super::*;
use crate::metadata::gpkg::{self, PreparedWrite, RecordKind};
use std::collections::BTreeSet;

pub(super) struct PreparedCollectionMetadata {
    resources: Vec<PreparedWrite>,
    global: PreparedWrite,
    manifest: PreparedWrite,
}

impl CdbDatastore {
    pub(super) fn prepare_gpkg_collection_metadata(
        &self,
        pending: &PendingCollection,
        global: &GlobalMetadata,
        manifest: &CollectionManifest,
    ) -> Result<PreparedCollectionMetadata, CdbError> {
        self.check_global_metadata_encoding(MetadataEncoding::Gpkg)?;
        let assets: BTreeSet<_> = pending
            .changes
            .iter()
            .map(|change| target_path(self.resolve(&change.asset)?).map_err(CdbError::from))
            .collect::<Result<_, _>>()?;
        let mut records = Vec::new();
        let mut seen = BTreeSet::new();
        for logical in pending
            .changes
            .iter()
            .filter_map(|change| change.resource_record.as_deref())
        {
            let path = target_path(self.resolve(logical)?)?;
            if assets.contains(&path) {
                return Err(MetadataError::UnsupportedContainer { reason: format!(
                    "managed metadata record {logical:?} is also an asset target in this collection") }.into());
            }
            match path_extension(logical).and_then(extension_encoding) {
                Some(MetadataEncoding::Gpkg) => {}
                Some(found) => {
                    return Err(
                        MetadataError::Violation(MetadataViolation::EncodingMismatch {
                            file: logical.to_owned(),
                            declared: MetadataEncoding::Gpkg,
                            found,
                        })
                        .into(),
                    );
                }
                None => {
                    return Err(MetadataError::Violation(MetadataViolation::Malformed {
                        reason: format!("linked record {logical:?} must have the gpkg extension"),
                    })
                    .into());
                }
            }
            if seen.insert(path.clone()) {
                records.push((logical, path));
            }
        }
        let mut resources = Vec::with_capacity(records.len());
        for (logical, path) in records {
            let mut resource = self.read_resource_metadata(logical)?;
            resource.updated = Some(manifest.applied);
            resource.validate().map_err(MetadataError::from)?;
            resources.push(gpkg::prepare_write(
                &path,
                RecordKind::Resource,
                &resource.to_json_string()?,
                manifest.applied,
            )?);
        }
        let mut global = global.clone();
        global.update = Some(manifest.applied);
        global.validate().map_err(MetadataError::from)?;
        let global_path = self
            .layout
            .global_metadata_dir()
            .join("global_metadata.gpkg");
        let global = gpkg::prepare_write(
            &global_path,
            RecordKind::Global,
            &global.to_json_string()?,
            manifest.applied,
        )?;
        let manifest_path = self.manifest_path(manifest.id, MetadataEncoding::Gpkg);
        let manifest = gpkg::prepare_write(
            &manifest_path,
            RecordKind::Collection,
            &manifest.to_json_string()?,
            manifest.applied,
        )?;
        Ok(PreparedCollectionMetadata {
            resources,
            global,
            manifest,
        })
    }
}

// Existing targets need their filesystem identity: case and symlink aliases
// must neither bypass overlap checks nor split one record into two writes.
// Only new asset targets may be absent during the preparation phase.
fn target_path(path: PathBuf) -> Result<PathBuf, MetadataError> {
    match fs::canonicalize(&path) {
        Ok(target) => Ok(target),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn publish_prepared_collection_metadata(
    prepared: PreparedCollectionMetadata,
    mut install: impl FnMut(PreparedWrite) -> Result<PathBuf, MetadataError>,
) -> Result<(), CdbError> {
    for resource in prepared.resources {
        install(resource)?;
    }
    install(prepared.global)?;
    install(prepared.manifest)?;
    Ok(())
}
