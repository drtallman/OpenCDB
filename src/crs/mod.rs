//! Coordinate Reference System requirements module.
//!
//! Implements the CDB Core CRS requirements class (spec §7.3,
//! `/req/core/data-representation`): one CRS per datastore (CRS3), geodetic
//! or geographic only (CRS4), WKT-2 metadata in the global metadata folder
//! (CRS5), uniform coordinate units (CRS6), and an epoch for dynamic
//! reference frames (CRS7). The optional Vertical CRS class lives in
//! [`vertical`]; the WKT-2 reader in [`wkt2`].

pub mod vertical;
pub mod wkt2;

use std::fmt;
use std::fs;
use std::path::PathBuf;

use thiserror::Error;

use crate::hierarchy::DatastoreLayout;
use wkt2::{CrsKind, WktNode};

pub use vertical::VerticalCrs;

/// File holding the storage CRS in `global_metadata/` (Requirement CRS5).
/// `.wkt` is the industry-standard extension (Requirement Name7-B).
pub const CRS_FILE_NAME: &str = "crs.wkt";

/// A violation of a SHALL requirement in the CRS module.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CrsViolation {
    #[error("invalid WKT-2 at byte {position}: {reason} (violates /req/core/crs/crsMetadata)")]
    InvalidWkt { position: usize, reason: String },
    #[error("{keyword} is not a coordinate reference system (violates /req/core/crs/crsMetadata)")]
    NotACrs { keyword: String },
    #[error(
        "storage CRS kind {kind:?} is not allowed; only non-projected geodetic or geographic \
         CRSs may be used (violates /req/core/crs/storageCrs-valid-value)"
    )]
    NonGeodeticStorageCrs { kind: CrsKind },
    #[error(
        "the first (horizontal) member of a compound storage CRS is {kind:?}, expected geodetic \
         or geographic (violates /req/core/crs/storageCrs-valid-value)"
    )]
    CompoundHorizontalNotGeodetic { kind: CrsKind },
    #[error(
        "coordinate units differ within CRS component {component:?}: {first} vs {second} \
         (violates /req/core/crs/uom)"
    )]
    InconsistentCoordinateUnits {
        component: String,
        first: String,
        second: String,
    },
    #[error(
        "the CRS declares a dynamic reference frame but no epoch is given \
         (violates /req/core/crs/crsEpoch)"
    )]
    MissingEpoch,
    #[error("{value:?} is not a valid epoch: {reason} (violates /req/core/crs/crsEpoch B)")]
    InvalidEpoch { value: String, reason: &'static str },
    #[error(
        "a CRS is already defined for this datastore ({existing:?}); only one CRS may be \
         specified per datastore (violates /req/core/crs/crsStorage)"
    )]
    CrsAlreadyDefined { existing: String },
    #[error("{keyword} is not a vertical CRS (violates /req/core/crs/vcrs-topic2)")]
    NotAVerticalCrs { keyword: String },
    #[error("no CRS metadata found at {searched:?} (violates /req/core/crs/crsMetadata)")]
    MissingCrsMetadata { searched: PathBuf },
}

/// A finding against a SHOULD recommendation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CrsWarning {
    /// `/rec/core/crs/crs-definition`: WGS-84 (EPSG:4326/4979) recommended
    /// for backwards compatibility.
    NotWgs84 { found: String },
}

impl fmt::Display for CrsWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CrsWarning::NotWgs84 { found } => write!(
                f,
                "storage CRS is {found}; WGS-84 (EPSG:4326/4979) is recommended \
                 (/rec/core/crs/crs-definition)"
            ),
        }
    }
}

/// Operational errors for CRS metadata I/O.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CrsError {
    #[error(transparent)]
    Violation(#[from] CrsViolation),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A coordinate epoch as a decimal Gregorian year (Requirement CRS7-B:
/// `yyyy.00` is midnight, 1 January of year `yyyy`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Epoch(f64);

impl Epoch {
    /// Accepts finite decimal years within the Gregorian calendar
    /// (1582.0 ≤ year < 10000.0 — an implementation sanity bound).
    pub fn new(decimal_year: f64) -> Result<Self, CrsViolation> {
        if !decimal_year.is_finite() {
            return Err(CrsViolation::InvalidEpoch {
                value: format!("{decimal_year}"),
                reason: "epoch must be a finite decimal year",
            });
        }
        if !(1582.0..10000.0).contains(&decimal_year) {
            return Err(CrsViolation::InvalidEpoch {
                value: format!("{decimal_year}"),
                reason: "epoch must be a Gregorian decimal year (1582.0..10000.0)",
            });
        }
        Ok(Self(decimal_year))
    }

    pub fn parse(text: &str) -> Result<Self, CrsViolation> {
        let value = text
            .parse::<f64>()
            .map_err(|_| CrsViolation::InvalidEpoch {
                value: text.to_owned(),
                reason: "epoch must be a decimal year such as 2017.53",
            })?;
        Self::new(value)
    }

    pub fn decimal_year(self) -> f64 {
        self.0
    }
}

impl fmt::Display for Epoch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.2}", self.0)
    }
}

/// The single storage CRS of a CDB datastore, validated at construction:
/// consistency with ISO 19111 is carried by the WKT-2 structure (CRS2);
/// only geodetic/geographic (or compound with such a horizontal) kinds are
/// accepted (CRS4); coordinate units are uniform (CRS6); dynamic frames
/// carry an epoch (CRS7).
#[derive(Debug, Clone, PartialEq)]
pub struct StorageCrs {
    canonical_wkt: String,
    root: WktNode,
    epoch: Option<Epoch>,
}

impl StorageCrs {
    /// Validates and adopts a WKT-2 CRS. A `COORDINATEMETADATA[crs,EPOCH[…]]`
    /// wrapper is unwrapped; an explicit `epoch` argument takes precedence
    /// over a wrapper epoch.
    pub fn new(wkt: &str, epoch: Option<Epoch>) -> Result<Self, CrsViolation> {
        let parsed = wkt2::parse(wkt)?;
        let (root, wrapper_epoch) = if parsed.keyword == "COORDINATEMETADATA" {
            let crs = parsed
                .child_nodes()
                .next()
                .cloned()
                .ok_or(CrsViolation::NotACrs {
                    keyword: "COORDINATEMETADATA".to_owned(),
                })?;
            let wrapper_epoch = parsed
                .find("EPOCH")
                .and_then(WktNode::first_number)
                .map(Epoch::new)
                .transpose()?;
            (crs, wrapper_epoch)
        } else {
            (parsed, None)
        };
        let epoch = epoch.or(wrapper_epoch);

        let kind = wkt2::classify(&root);
        let components: Vec<&WktNode> = match &kind {
            CrsKind::Geodetic | CrsKind::Geographic => vec![&root],
            CrsKind::Compound => {
                let members: Vec<&WktNode> = root
                    .child_nodes()
                    .filter(|n| wkt2::is_crs_node(n))
                    .collect();
                let first_kind =
                    members
                        .first()
                        .map(|m| wkt2::classify(m))
                        .ok_or(CrsViolation::NotACrs {
                            keyword: "COMPOUNDCRS".to_owned(),
                        })?;
                if !matches!(first_kind, CrsKind::Geodetic | CrsKind::Geographic) {
                    return Err(CrsViolation::CompoundHorizontalNotGeodetic { kind: first_kind });
                }
                for member in &members {
                    let member_kind = wkt2::classify(member);
                    if matches!(
                        member_kind,
                        CrsKind::Projected | CrsKind::Engineering | CrsKind::Compound
                    ) {
                        return Err(CrsViolation::NonGeodeticStorageCrs { kind: member_kind });
                    }
                }
                members
            }
            CrsKind::Other(keyword) => {
                return Err(CrsViolation::NotACrs {
                    keyword: keyword.clone(),
                });
            }
            other => {
                return Err(CrsViolation::NonGeodeticStorageCrs {
                    kind: other.clone(),
                });
            }
        };

        // Requirement CRS6: uniform coordinate units per component.
        for component in &components {
            wkt2::check_coordinate_units(component)?;
        }

        // Requirement CRS7: a dynamic reference frame needs an epoch, either
        // the datastore epoch or a FRAMEEPOCH in the WKT itself.
        let dynamic = components.iter().any(|c| wkt2::is_dynamic(c));
        let has_frame_epoch = components.iter().any(|c| wkt2::frame_epoch(c).is_some());
        if dynamic && epoch.is_none() && !has_frame_epoch {
            return Err(CrsViolation::MissingEpoch);
        }

        drop(components);
        Ok(Self {
            canonical_wkt: root.to_string(),
            root,
            epoch,
        })
    }

    /// Reads a WKT-2 text, honoring a `COORDINATEMETADATA` wrapper.
    pub fn from_wkt(text: &str) -> Result<Self, CrsViolation> {
        Self::new(text, None)
    }

    /// The CRS in canonical WKT-2; wrapped in `COORDINATEMETADATA` with an
    /// `EPOCH` when a datastore epoch is set.
    pub fn to_wkt(&self) -> String {
        match self.epoch {
            Some(epoch) => format!("COORDINATEMETADATA[{},EPOCH[{epoch}]]", self.canonical_wkt),
            None => self.canonical_wkt.clone(),
        }
    }

    pub fn kind(&self) -> CrsKind {
        wkt2::classify(&self.root)
    }

    /// The horizontal CRS: the first member for a compound, otherwise the
    /// CRS itself.
    pub fn horizontal(&self) -> &WktNode {
        if matches!(self.kind(), CrsKind::Compound) {
            self.root
                .child_nodes()
                .find(|n| wkt2::is_crs_node(n))
                .unwrap_or(&self.root)
        } else {
            &self.root
        }
    }

    pub fn name(&self) -> Option<&str> {
        self.horizontal().name()
    }

    pub fn authority(&self) -> Option<(String, String)> {
        wkt2::authority_id(self.horizontal())
    }

    pub fn epoch(&self) -> Option<Epoch> {
        self.epoch
    }

    pub fn is_dynamic(&self) -> bool {
        if matches!(self.kind(), CrsKind::Compound) {
            self.root.child_nodes().any(wkt2::is_dynamic)
        } else {
            wkt2::is_dynamic(&self.root)
        }
    }

    /// SHOULD-level findings: `/rec/core/crs/crs-definition` recommends
    /// WGS-84 (EPSG:4326 in 2D, EPSG:4979 in 3D).
    pub fn warnings(&self) -> Vec<CrsWarning> {
        match self.authority() {
            Some((authority, code))
                if authority.eq_ignore_ascii_case("EPSG") && (code == "4326" || code == "4979") =>
            {
                Vec::new()
            }
            Some((authority, code)) => vec![CrsWarning::NotWgs84 {
                found: format!("{authority}:{code}"),
            }],
            None => vec![CrsWarning::NotWgs84 {
                found: self.name().unwrap_or_default().to_owned(),
            }],
        }
    }

    /// Writes the CRS metadata into the datastore's global metadata folder
    /// (Requirement CRS5). Enforces Requirement CRS3: writing a *different*
    /// CRS over an existing one is a violation; rewriting the same CRS is
    /// idempotent.
    pub fn write_to(&self, layout: &DatastoreLayout) -> Result<PathBuf, CrsError> {
        let dir = layout.global_metadata_dir();
        let path = dir.join(CRS_FILE_NAME);
        if path.is_file() {
            let existing = Self::read_from(layout)?;
            if existing != *self {
                return Err(CrsError::Violation(CrsViolation::CrsAlreadyDefined {
                    existing: existing.name().unwrap_or_default().to_owned(),
                }));
            }
        }
        fs::create_dir_all(&dir)?;
        fs::write(&path, self.to_wkt())?;
        Ok(path)
    }

    /// Reads the datastore's storage CRS (Requirement CRS5).
    pub fn read_from(layout: &DatastoreLayout) -> Result<Self, CrsError> {
        let path = layout.global_metadata_dir().join(CRS_FILE_NAME);
        if !path.is_file() {
            return Err(CrsViolation::MissingCrsMetadata { searched: path }.into());
        }
        let text = fs::read_to_string(&path)?;
        Ok(Self::from_wkt(&text)?)
    }
}

/// Compares two CRS authority identities (authority, code) ASCII
/// case-insensitively on both components. Shared by the geometry (Geom5)
/// and coverage (Coverages4) CRS-association checks.
pub(crate) fn authority_ids_match(a: &(String, String), b: &(String, String)) -> bool {
    a.0.eq_ignore_ascii_case(&b.0) && a.1.eq_ignore_ascii_case(&b.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// The spec's compound-CRS example — **verbatim, with no correction at
    /// all**, including its missing comma between member CRSs and the space
    /// before `[`, both of which the reader is lenient about.
    ///
    /// The draft prints this example twice. The copy taken here is
    /// **§7.3.1.4**'s (spec line 989), under Requirement CRS5, because it is
    /// the *balanced* one; §7.3.1.9's copy (line 1087) is byte-identical
    /// apart from the title string and closes `VERTCRS` and then stops,
    /// leaving `COMPOUNDCRS` open. An unclosed node is not a leniency
    /// question but an unreadable document, so a fixture built on it would
    /// have had to be corrected and would prove nothing about the reader.
    /// Taking the other copy whole avoids the correction entirely — the title
    /// `"I3S Compound CRS"` is the only difference, and a CRS's name is not
    /// what any of these tests judge. Errata §7 row 16.
    const SPEC_COMPOUND: &str = r#"COMPOUNDCRS ["I3S Compound CRS",
GEODCRS["WGS 84",
  DATUM["World Geodetic System 1984",
    ELLIPSOID["WGS 84",6378137,298.257223563,LENGTHUNIT["metre",1.0]]],
  CS[ellipsoidal,2],
    AXIS["latitude",north,ORDER[1]],
    AXIS["longitude",east,ORDER[2]],
    ANGLEUNIT["degree",0.01745329252],
  ID["EPSG",4326]]
VERTCRS["EGM2008 height",
  VDATUM["EGM2008 geoid"],
  CS[vertical,1],
    AXIS["gravity-related height (H)",up],
    LENGTHUNIT["metre",1.0],
  ID["EPSG",3855]]]"#;

    /// The spec's static geographic example (§7.3.1.4), verbatim.
    const SPEC_NTF_PARIS: &str = r#"GEOGCRS["NTF (Paris)",
  DATUM["Nouvelle Triangulation Francaise",
    ELLIPSOID["Clarke 1880 (IGN)",6378249.2,293.4660213]
  ],

  PRIMEM["Paris",2.5969213],
  CS[ellipsoidal,2],
    AXIS["latitude",north,ORDER[1]],
    AXIS["longitude",east,ORDER[2]],
    ANGLEUNIT["grad",0.015707963267949],
  REMARK["Nouvelle Triangulation Française"]

]"#;

    const DYNAMIC_WGS84: &str = r#"GEOGCRS["WGS 84 (G1762)",
  DYNAMIC[FRAMEEPOCH[2005.0]],
  DATUM["World Geodetic System 1984 (G1762)",
    ELLIPSOID["WGS 84",6378137,298.257223563,LENGTHUNIT["metre",1.0]]],
  CS[ellipsoidal,2],
    AXIS["latitude",north],
    AXIS["longitude",east],
    ANGLEUNIT["degree",0.0174532925199433],
  ID["EPSG",9057]]"#;

    const DYNAMIC_NO_EPOCH: &str = r#"GEOGCRS["Dynamic no epoch",
  DYNAMIC[],
  DATUM["D",ELLIPSOID["WGS 84",6378137,298.257223563]],
  CS[ellipsoidal,2],
    AXIS["latitude",north],
    AXIS["longitude",east],
    ANGLEUNIT["degree",0.0174532925199433]]"#;

    const PROJECTED: &str = r#"PROJCRS["WGS 84 / UTM zone 31N",
  BASEGEOGCRS["WGS 84",DATUM["World Geodetic System 1984",
    ELLIPSOID["WGS 84",6378137,298.257223563]]],
  CONVERSION["UTM zone 31N",METHOD["Transverse Mercator"]],
  CS[Cartesian,2],AXIS["(E)",east],AXIS["(N)",north],
  LENGTHUNIT["metre",1.0],
  ID["EPSG",32631]]"#;

    const MIXED_UNITS: &str = r#"GEOGCRS["Mixed",
  DATUM["D",ELLIPSOID["E",6378137,298.25]],
  CS[ellipsoidal,2],
    AXIS["latitude",north,ANGLEUNIT["degree",0.0174532925199433]],
    AXIS["longitude",east,ANGLEUNIT["grad",0.015707963267949]]]"#;

    /// §7.3.1.3 Requirement CRS4 — geographic/geodetic accepted.
    #[test]
    fn req_core_crs_storage_valid_value_accepts_geographic() {
        let crs = StorageCrs::new(SPEC_NTF_PARIS, None).unwrap();
        assert_eq!(crs.kind(), wkt2::CrsKind::Geographic);
        assert_eq!(crs.name(), Some("NTF (Paris)"));
        assert!(!crs.is_dynamic());
    }

    /// §7.3.1.3 Requirement CRS4 — projected/engineering/vertical-only rejected.
    #[test]
    fn req_core_crs_storage_valid_value_rejects_non_geodetic() {
        assert!(matches!(
            StorageCrs::new(PROJECTED, None),
            Err(CrsViolation::NonGeodeticStorageCrs {
                kind: wkt2::CrsKind::Projected
            })
        ));
        assert!(matches!(
            StorageCrs::new(r#"ENGCRS["local",EDATUM["site"]]"#, None),
            Err(CrsViolation::NonGeodeticStorageCrs {
                kind: wkt2::CrsKind::Engineering
            })
        ));
        assert!(matches!(
            StorageCrs::new(r#"VERTCRS["h",VDATUM["d"]]"#, None),
            Err(CrsViolation::NonGeodeticStorageCrs {
                kind: wkt2::CrsKind::Vertical
            })
        ));
        assert!(matches!(
            StorageCrs::new(r#"DATUM["not a crs"]"#, None),
            Err(CrsViolation::NotACrs { .. })
        ));
    }

    /// §7.3.1.4/§7.3.1.9 Requirement CRS5 — the spec's own compound example
    /// parses verbatim; the horizontal member is EPSG:4326.
    #[test]
    fn req_core_crs_metadata_spec_compound_example_verbatim() {
        let crs = StorageCrs::new(SPEC_COMPOUND, None).unwrap();
        assert_eq!(crs.kind(), wkt2::CrsKind::Compound);
        assert_eq!(crs.name(), Some("WGS 84"));
        assert_eq!(crs.authority(), Some(("EPSG".into(), "4326".into())));
        assert!(crs.warnings().is_empty());
    }

    /// §7.3.1.4 Requirement CRS5 — WKT-2 file round-trip through the
    /// global metadata folder.
    #[test]
    fn req_core_crs_metadata_file_roundtrip() {
        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        let crs = StorageCrs::new(SPEC_COMPOUND, None).unwrap();

        let path = crs.write_to(&layout).unwrap();
        assert!(path.ends_with("global_metadata/crs.wkt"));
        assert_eq!(StorageCrs::read_from(&layout).unwrap(), crs);
    }

    /// §7.3.1.2 Requirement CRS3 — only one CRS per datastore.
    #[test]
    fn req_core_crs_storage_one_crs_per_datastore() {
        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        let wgs84 = StorageCrs::new(SPEC_COMPOUND, None).unwrap();
        wgs84.write_to(&layout).unwrap();
        wgs84.write_to(&layout).unwrap(); // idempotent rewrite is fine

        let ntf = StorageCrs::new(SPEC_NTF_PARIS, None).unwrap();
        assert!(matches!(
            ntf.write_to(&layout),
            Err(CrsError::Violation(CrsViolation::CrsAlreadyDefined { .. }))
        ));
    }

    /// §7.3.1.5 Requirement CRS6 — coordinate units must be uniform.
    #[test]
    fn req_core_crs_uom_mismatch_detected() {
        assert!(matches!(
            StorageCrs::new(MIXED_UNITS, None),
            Err(CrsViolation::InconsistentCoordinateUnits { .. })
        ));
    }

    /// §7.3.1.6 Requirement CRS7 — dynamic reference frames need an epoch.
    #[test]
    fn req_core_crs_epoch_dynamic_requires_epoch() {
        // FRAMEEPOCH inside DYNAMIC satisfies the requirement.
        let with_frame = StorageCrs::new(DYNAMIC_WGS84, None).unwrap();
        assert!(with_frame.is_dynamic());

        assert!(matches!(
            StorageCrs::new(DYNAMIC_NO_EPOCH, None),
            Err(CrsViolation::MissingEpoch)
        ));
        let with_epoch =
            StorageCrs::new(DYNAMIC_NO_EPOCH, Some(Epoch::new(2017.53).unwrap())).unwrap();
        assert_eq!(with_epoch.epoch().unwrap().decimal_year(), 2017.53);
    }

    /// §7.3.1.6 CRS7 — the epoch round-trips via COORDINATEMETADATA.
    #[test]
    fn req_core_crs_epoch_roundtrips_through_coordinatemetadata() {
        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        let crs = StorageCrs::new(DYNAMIC_NO_EPOCH, Some(Epoch::new(2017.53).unwrap())).unwrap();

        let path = crs.write_to(&layout).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("COORDINATEMETADATA["), "{content}");
        assert!(content.contains("EPOCH[2017.53]"), "{content}");

        let back = StorageCrs::read_from(&layout).unwrap();
        assert_eq!(back, crs);
        assert_eq!(back.epoch().unwrap().decimal_year(), 2017.53);
    }

    /// §7.3.1.6 CRS7-B — the epoch is a decimal Gregorian year.
    #[test]
    fn req_core_crs_epoch_decimal_year_format() {
        assert_eq!(Epoch::parse("2017.53").unwrap().to_string(), "2017.53");
        assert_eq!(Epoch::new(2024.0).unwrap().to_string(), "2024.00");
        for bad in ["abc", "2017-06"] {
            assert!(matches!(
                Epoch::parse(bad),
                Err(CrsViolation::InvalidEpoch { .. })
            ));
        }
        assert!(matches!(
            Epoch::new(f64::NAN),
            Err(CrsViolation::InvalidEpoch { .. })
        ));
        assert!(matches!(
            Epoch::new(1066.0),
            Err(CrsViolation::InvalidEpoch { .. })
        ));
    }

    /// §7.3.1.3 Recommendation `/rec/core/crs/crs-definition` — WGS-84
    /// recommended: EPSG:4326 silent, anything else warns.
    #[test]
    fn rec_core_crs_definition_wgs84_recommended() {
        assert!(
            StorageCrs::new(SPEC_COMPOUND, None)
                .unwrap()
                .warnings()
                .is_empty()
        );
        assert_eq!(
            StorageCrs::new(SPEC_NTF_PARIS, None).unwrap().warnings(),
            vec![CrsWarning::NotWgs84 {
                found: "NTF (Paris)".into()
            }]
        );
    }

    #[test]
    fn crs_error_converts_to_cdb_error() {
        let error: crate::CdbError = CrsError::from(CrsViolation::MissingEpoch).into();
        assert!(matches!(error, crate::CdbError::Crs(_)));
    }

    /// Shared CRS-authority identity comparison used by the geometry
    /// (Geom5) and coverage (Coverages4) modules: ASCII case-insensitive
    /// on both the authority and the code.
    #[test]
    fn authority_ids_match_is_ascii_case_insensitive() {
        let a = ("EPSG".to_owned(), "4326".to_owned());
        let b = ("epsg".to_owned(), "4326".to_owned());
        let c = ("EPSG".to_owned(), "4269".to_owned());
        assert!(authority_ids_match(&a, &b));
        assert!(authority_ids_match(&a, &a));
        assert!(!authority_ids_match(&a, &c));
    }
}
