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
            .map(|change| self.resolve(&change.asset))
            .collect::<Result<_, _>>()?;
        let mut records = Vec::new();
        let mut seen = BTreeSet::new();
        for logical in pending
            .changes
            .iter()
            .filter_map(|change| change.resource_record.as_deref())
        {
            let path = self.resolve(logical)?;
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
