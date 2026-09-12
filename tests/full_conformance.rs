//! Integration: the **full conformance round-trip** (Annex A
//! `/conf/minimal-core` plus all six optional requirements classes).
//!
//! Phases 1–13 built one requirements module at a time and one integration
//! test per module. This file is the composite the plan reserved for 14b:
//! create → write vector-tile payloads at real tile addresses under the real
//! hierarchy, each with its resource metadata record → write an elevation
//! coverage carrying a `domainSet` → write an attribute model → apply
//! versioning collections → **reopen** → read everything back, asserting
//! byte equality on the payloads, value equality on every structure the
//! crate owns, and a `validate()` that reports every declared class
//! conformant with zero warnings.
//!
//! **Payloads are opaque byte blobs.** The crate deliberately decodes
//! neither GeoPackage containers nor raster files (`proj`/`gdal` are absent
//! by design), so a "vector tile" here is bytes at a conformant tile address
//! with conformant metadata beside it. Everything the crate actually owns —
//! addresses, CRS, domain sets, metadata records, the attribute model, the
//! versioning journal — round-trips for real.
//!
//! Three passes:
//!
//! 1. [`req_core_conformance_full_roundtrip_simulation_profile`] — the
//!    default profile over CDB1GlobalGrid (§7.11) tile addresses;
//! 2. [`req_core_conformance_full_roundtrip_gnosis_profile`] — the same
//!    datastore shape under the GNOSIS-declaring profile over
//!    GNOSISGlobalGrid (§7.12) addresses, which is what makes the second
//!    profile earn its place: the `ApplicationProfile` trait's premise is
//!    that profiles vary, and only a second profile tests it;
//! 3. [`req_core_conformance_restricted_profile_reports_declaration_mismatch`]
//!    — a profile declaring *less* than its datastore holds, which is the
//!    content sweep's integration-level coverage now that both shipped
//!    profiles declare every class.

use std::fs;

use chrono::{DateTime, Utc};
use rusty_cdb::attribution::{AttributeDef, AttributeModel};
use rusty_cdb::conformance::{CdbViolation, RequirementsClass};
use rusty_cdb::coverage::DomainSet;
use rusty_cdb::crs::{CrsViolation, StorageCrs};
use rusty_cdb::links::Link;
use rusty_cdb::metadata::{MetadataEncoding, MetadataStandard, ResourceMetadata, UnitOfMeasure};
use rusty_cdb::naming::StyleGuide;
use rusty_cdb::profiles::{ApplicationProfile, StorageTechnology, TilingSchemeId};
use rusty_cdb::tiling::{
    Cdb1GlobalGrid, Cdb1Lod, Cdb1TileAddress, GnosisGlobalGrid, GnosisLevel, GnosisTileAddress,
    TilingScheme,
};
use rusty_cdb::topology::WindingOrder;
use rusty_cdb::versioning::{ChangeAction, CollectionId, PendingCollection};
use rusty_cdb::{CdbDatastore, DatastoreSeed, GnosisProfile, SimulationProfile};

/// The instant the initial content collection is applied (V3-A).
const APPLIED_LOAD: &str = "2026-09-12T09:00:00Z";
/// The instant the roadworks collection is applied (V3-A).
const APPLIED_ROADWORKS: &str = "2026-09-12T10:00:00Z";

/// A point in California and a point in New South Wales: two hemispheres, so
/// the tile addressing is exercised north and south of the equator.
const POINT_A: (f64, f64) = (37.25, -122.75);
const POINT_B: (f64, f64) = (-33.85, 151.20);

fn ts(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

/// One datastore asset: an opaque payload blob and the resource metadata
/// record that describes it.
struct Fixture {
    /// Logical path of the payload (the opaque blob).
    payload: String,
    /// Logical path of its resource metadata record (the profile's Name5
    /// convention path).
    record: String,
    /// Bytes written by the initial content collection.
    bytes: Vec<u8>,
    /// Bytes written by a later collection, when one replaces this asset.
    replacement: Option<Vec<u8>>,
    /// The record as authored, before the datastore stamps it.
    metadata: ResourceMetadata,
    /// The instant the last collection touching this asset was applied —
    /// what V3-C puts in the record's `updated` element.
    updated: DateTime<Utc>,
}

impl Fixture {
    /// The bytes that must be on disk at the end of the round-trip.
    fn expected_bytes(&self) -> &[u8] {
        self.replacement.as_deref().unwrap_or(&self.bytes)
    }

    /// The record as it must read back: what was authored, plus the V3-C
    /// `updated` stamp the apply pipeline adds.
    fn expected_record(&self) -> ResourceMetadata {
        let mut expected = self.metadata.clone();
        expected.updated = Some(self.updated);
        expected
    }
}

/// The spec's §7.1.2.3 example attribute model (Requirement Attr2), the same
/// fixture `tests/attribution_roundtrip.rs` uses.
fn street_model() -> AttributeModel {
    AttributeModel {
        schema_uri: Some("https://example.org/schemas/street.xsd".to_owned()),
        attributes: vec![
            AttributeDef {
                id: "1".to_owned(),
                name: "StreetName".to_owned(),
                description: "Name of a street as an alphanumeric string".to_owned(),
            },
            AttributeDef {
                id: "2".to_owned(),
                name: "StreetType".to_owned(),
                description: "Type street as an alphanumeric string (Interstate, Arterial, . . .)"
                    .to_owned(),
            },
        ],
    }
}

/// A vector-tile record: Tiling10 keywords (mandatory on every record of a
/// tiling-declaring datastore — see `docs/CONFORMANCE.md`), an association
/// link to the payload it describes (Requirements Link1/Link2), and the
/// Geom4 `uom` conditional element declaring the unit of its m coordinates.
fn tile_record(id: &str, title: &str, payload: &str) -> ResourceMetadata {
    let mut record = ResourceMetadata::new(id, title, "Vector tile payload");
    record.keywords = vec!["vector".to_owned(), "tile".to_owned()];
    record.uom = Some(UnitOfMeasure::Meters);
    record.associations = vec![
        Link::new(payload, "describes")
            .unwrap()
            .with_media_type("application/geopackage+sqlite3")
            .with_title(title),
    ];
    record
}

/// An elevation-coverage record: Coverages6's `domainSet` (metres, the
/// §7.2.6 defaults) plus the Tiling10 keywords.
fn coverage_record(id: &str, payload: &str) -> ResourceMetadata {
    let mut record = ResourceMetadata::new(id, "Terrain Elevation", "Gridded DEM tileset");
    record.keywords = vec!["elevation".to_owned(), "terrain".to_owned()];
    record.domain_set = Some(DomainSet::new("m"));
    record.associations = vec![Link::new(payload, "describes").unwrap()];
    record
}

/// A topology-bearing record: the Face4 `windingOrder` conditional element
/// (§7.13.5.5) plus the Tiling10 keywords.
fn topology_record(id: &str, payload: &str) -> ResourceMetadata {
    let mut record = ResourceMetadata::new(id, "Road Network Faces", "Topologically structured");
    record.keywords = vec!["topology".to_owned(), "faces".to_owned()];
    record.winding_order = Some(WindingOrder::Counterclockwise);
    record.associations = vec![Link::new(payload, "describes").unwrap()];
    record
}

/// Builds the datastore: create → Tiling8 scheme on the global record →
/// resource metadata records → attribute model → two versioning
/// collections. Returns the temporary directory that owns the root, so the
/// caller can reopen it.
///
/// Payloads are created through a versioning collection (V4-A) rather than
/// written behind the datastore's back: that is the crate's only
/// content-writing API, and it makes the journal part of the round-trip
/// rather than an afterthought.
fn build(
    profile: &dyn ApplicationProfile,
    scheme: TilingScheme,
    fixtures: &[Fixture],
) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let seed = DatastoreSeed::new(
        "doi:10.5066/cdb.full",
        "Full Conformance Store",
        "Every requirements class, end to end",
        "ops@example.com",
    );
    let store = CdbDatastore::create(tmp.path(), profile, seed).unwrap();

    // Requirement Tiling8 (§7.10.2.8): the scheme definition rides the
    // global metadata record's `tilingScheme` conditional element.
    let mut global = store.global_metadata().unwrap();
    global.tiling_scheme = Some(scheme);
    store.write_global_metadata(&global).unwrap();

    // Requirement Attr1-B/C (§7.1.2.1): the model lands in
    // `global_metadata/vector_attributes.<declared encoding>`.
    store.write_attribute_model(&street_model()).unwrap();

    // The records must exist before a collection may link to them: V3-C
    // refreshes a linked record's `updated` element, which presupposes one.
    for fixture in fixtures {
        store
            .write_resource_metadata(&fixture.record, &fixture.metadata)
            .unwrap();
    }

    // V4-A: one collection creating every payload, each linked to its record.
    let mut load = PendingCollection::new().description("initial tile load");
    for fixture in fixtures {
        load = load
            .create(&fixture.payload, fixture.bytes.clone())
            .for_record(&fixture.record);
    }
    store.apply_collection_at(load, ts(APPLIED_LOAD)).unwrap();

    // V4-C/V5: a second collection replacing whichever payloads carry one.
    // A fixture set with no replacement applies no second collection — an
    // empty collection is a `VersioningViolation::EmptyCollection`, and the
    // journal assertions read the fixture set the same way.
    let mut roadworks = PendingCollection::new().description("roadworks");
    let mut replacements = 0;
    for fixture in fixtures {
        if let Some(bytes) = fixture.replacement.as_ref() {
            roadworks = roadworks
                .replace(&fixture.payload, bytes.clone())
                .for_record(&fixture.record);
            replacements += 1;
        }
    }
    if replacements > 0 {
        store
            .apply_collection_at(roadworks, ts(APPLIED_ROADWORKS))
            .unwrap();
    }

    tmp
}

/// Reopens the datastore and reads everything back, asserting byte equality
/// on the payloads and value equality on every structure the crate owns.
fn read_back(root: &std::path::Path, profile: &dyn ApplicationProfile, fixtures: &[Fixture]) {
    let store = CdbDatastore::open(root).unwrap();

    // Payload bytes: opaque in, identical out.
    for fixture in fixtures {
        let physical = store.resolve(&fixture.payload).unwrap();
        assert_eq!(
            fs::read(&physical).unwrap(),
            fixture.expected_bytes(),
            "payload {} did not round-trip byte-for-byte",
            fixture.payload
        );
    }

    // Resource metadata records: every element, including the V3-C stamp.
    for fixture in fixtures {
        let back = store.read_resource_metadata(&fixture.record).unwrap();
        assert_eq!(
            back,
            fixture.expected_record(),
            "record {} did not round-trip",
            fixture.record
        );
    }

    // The Coverages6 domainSet survives with its §7.2.6 defaults intact.
    let coverage = fixtures
        .iter()
        .find(|fixture| fixture.metadata.domain_set.is_some())
        .expect("one coverage fixture");
    let domain_set = store
        .read_resource_metadata(&coverage.record)
        .unwrap()
        .domain_set
        .expect("domainSet element");
    assert_eq!(domain_set, DomainSet::new("m"));

    // Requirement CRS5: the storage CRS is the profile's, verbatim.
    assert_eq!(store.storage_crs().unwrap(), profile.storage_crs().unwrap());

    // The global record: the profile's policy, the Tiling8 element, and the
    // V3-B `update` stamp from the last collection.
    let global = store.global_metadata().unwrap();
    assert_eq!(global.encoding, profile.metadata_encoding());
    assert_eq!(global.metadata_standard, profile.metadata_standard());
    assert_eq!(global.uom, profile.uom());
    assert_eq!(global.language, profile.language().unwrap());
    assert_eq!(global.update, Some(ts(APPLIED_ROADWORKS)));
    let on_disk_scheme = global.tiling_scheme.expect("tilingScheme element");
    assert_eq!(
        Some(on_disk_scheme.id.as_str()),
        profile.tiling_scheme().map(TilingSchemeId::as_str),
        "the datastore's scheme must be the one the profile pins (Tiling4)"
    );

    // Requirement Attr1/Attr2: the attribute model, unchanged.
    assert_eq!(store.attribute_model().unwrap(), Some(street_model()));

    // The journal: two contiguous collections, every change recorded.
    let journal = store.versions().unwrap();
    assert_eq!(journal.len(), 2);
    assert_eq!(journal[0].id, CollectionId::parse("v000001").unwrap());
    assert_eq!(journal[0].sequence, 1);
    assert_eq!(journal[0].applied, ts(APPLIED_LOAD));
    assert_eq!(journal[0].description.as_deref(), Some("initial tile load"));
    assert_eq!(journal[0].changes.len(), fixtures.len());
    for (change, fixture) in journal[0].changes.iter().zip(fixtures) {
        assert_eq!(change.asset, fixture.payload);
        assert_eq!(change.action, ChangeAction::Created);
        assert_eq!(change.resource_record.as_deref(), Some(&*fixture.record));
        assert!(!change.archived, "a created asset has no prior bytes");
    }
    let replaced: Vec<&Fixture> = fixtures
        .iter()
        .filter(|fixture| fixture.replacement.is_some())
        .collect();
    assert_eq!(journal[1].id, CollectionId::parse("v000002").unwrap());
    assert_eq!(journal[1].applied, ts(APPLIED_ROADWORKS));
    assert_eq!(journal[1].changes.len(), replaced.len());
    for (change, fixture) in journal[1].changes.iter().zip(&replaced) {
        assert_eq!(change.asset, fixture.payload);
        assert_eq!(change.action, ChangeAction::Replaced);
        assert!(change.archived, "replaced bytes are archived (V5)");
    }
    // The archived prior bytes are the ones the first collection wrote.
    for fixture in &replaced {
        let archived = store
            .root()
            .join("versions/v000002/archive")
            .join(fixture.payload.trim_start_matches('/'));
        assert_eq!(fs::read(&archived).unwrap(), fixture.bytes);
    }
}

/// Validates the reopened datastore and asserts the exit state every pass
/// must reach: all eleven classes conformant, **zero** warnings, and every
/// class carrying real content rather than passing vacuously.
fn assert_fully_conformant(root: &std::path::Path, profile: &dyn ApplicationProfile) {
    let store = CdbDatastore::open(root).unwrap();
    let report = store.validate(profile).unwrap();
    assert!(report.is_conformant(), "report: {report}");
    for class in RequirementsClass::ALL {
        assert!(
            report.class_passed(class),
            "{class} must pass: {:?}",
            report.violations(class)
        );
        assert!(
            report.warnings(class).is_empty(),
            "{class} must have no warnings: {:?}",
            report.warnings(class)
        );
        assert!(
            report.class_has_content(class),
            "{class} must have been checked against real content, not pass vacuously"
        );
    }
}

/// Requirement TCE6/TCE7 (§7.11.3): a CDB1GlobalGrid tile's logical name
/// carries its matrix row and column, so an address survives a trip through
/// the file hierarchy.
fn cdb1_name(stem: &str, address: Cdb1TileAddress) -> String {
    format!(
        "/Tiles/L{:02}/{stem}R{}C{}",
        address.lod().value(),
        address.row(),
        address.col()
    )
}

/// Reads a CDB1 row/column back out of a logical path written by
/// [`cdb1_name`].
fn parse_cdb1_name(path: &str) -> (u64, u64) {
    let stem = path.rsplit('/').next().unwrap().split('.').next().unwrap();
    let (before, col) = stem.rsplit_once('C').unwrap();
    let (_, row) = before.rsplit_once('R').unwrap();
    (row.parse().unwrap(), col.parse().unwrap())
}

/// §7.12.2: a GNOSISGlobalGrid tile's logical name carries its 64-bit key.
fn gnosis_name(stem: &str, address: GnosisTileAddress) -> String {
    format!(
        "/Tiles/G{:02}/{stem}K{}",
        address.level().value(),
        address.key()
    )
}

/// Reads a GNOSIS 64-bit key back out of a logical path written by
/// [`gnosis_name`].
fn parse_gnosis_name(path: &str) -> u64 {
    let stem = path.rsplit('/').next().unwrap().split('.').next().unwrap();
    stem.rsplit_once('K').unwrap().1.parse().unwrap()
}

/// The full round-trip under the default `simulation` profile over
/// CDB1GlobalGrid (§7.11) tile addresses: Annex A `/conf/minimal-core` plus
/// all six optional classes, create → write → apply → reopen → read back.
#[test]
fn req_core_conformance_full_roundtrip_simulation_profile() {
    let profile = SimulationProfile::json();
    let lod = Cdb1Lod::new(3).unwrap();
    let address_a = Cdb1GlobalGrid::tile_at(POINT_A.0, POINT_A.1, lod).unwrap();
    let address_b = Cdb1GlobalGrid::tile_at(POINT_B.0, POINT_B.1, lod).unwrap();

    let roads = format!("{}.gpkg", cdb1_name("RoadNetwork", address_a));
    let faces = format!("{}.gpkg", cdb1_name("RoadFaces", address_b));
    let elevation = format!("{}.tif", cdb1_name("Elevation", address_a));
    let fixtures = vec![
        Fixture {
            record: profile.resource_metadata_path(&roads).unwrap(),
            metadata: tile_record("RoadNetwork", "Road Network", &roads),
            bytes: vec![0, 159, 146, 150, 255, 1, 2, 3],
            replacement: Some(vec![255, 0, 128, 7, 7, 7]),
            updated: ts(APPLIED_ROADWORKS),
            payload: roads.clone(),
        },
        Fixture {
            record: profile.resource_metadata_path(&faces).unwrap(),
            metadata: topology_record("RoadFaces", &faces),
            bytes: b"faces-v1".to_vec(),
            replacement: None,
            updated: ts(APPLIED_LOAD),
            payload: faces.clone(),
        },
        Fixture {
            record: profile.resource_metadata_path(&elevation).unwrap(),
            metadata: coverage_record("Elevation", &elevation),
            bytes: vec![b'I', b'I', 42, 0, 8, 0, 0, 0],
            replacement: None,
            updated: ts(APPLIED_LOAD),
            payload: elevation.clone(),
        },
    ];

    let tmp = build(&profile, TilingScheme::cdb1_global_grid(), &fixtures);
    let root = tmp.path().join(profile.root_folder_name());
    read_back(&root, &profile, &fixtures);
    assert_fully_conformant(&root, &profile);

    // The tile addresses round-trip through the hierarchy: the row and
    // column read back off the on-disk name rebuild the very address the
    // grid computed for the point, and its extent contains that point.
    for (path, point, address) in [
        (&roads, POINT_A, address_a),
        (&faces, POINT_B, address_b),
        (&elevation, POINT_A, address_a),
    ] {
        let (row, col) = parse_cdb1_name(path);
        let rebuilt = Cdb1GlobalGrid::address(lod, row, col).unwrap();
        assert_eq!(rebuilt, address);
        assert_eq!(
            rebuilt,
            Cdb1GlobalGrid::tile_at(point.0, point.1, lod).unwrap()
        );
        let extent = Cdb1GlobalGrid::tile_extent(rebuilt);
        assert!(extent.west <= point.1 && point.1 < extent.east);
        assert!(extent.south <= point.0 && point.0 < extent.north);
    }
}

/// The same round-trip under the `gnosis` profile over GNOSISGlobalGrid
/// (§7.12) addresses. The `ApplicationProfile` trait's design premise is
/// that profiles vary (§5.1); this pass is what tests it end to end — the
/// facade is driven by the declaration, not by one implementation of it.
#[test]
fn req_core_conformance_full_roundtrip_gnosis_profile() {
    let profile = GnosisProfile::xml();
    let level = GnosisLevel::new(3).unwrap();
    let address_a = GnosisGlobalGrid::tile_at(POINT_A.0, POINT_A.1, level).unwrap();
    let address_b = GnosisGlobalGrid::tile_at(POINT_B.0, POINT_B.1, level).unwrap();

    let roads = format!("{}.gpkg", gnosis_name("RoadNetwork", address_a));
    let faces = format!("{}.gpkg", gnosis_name("RoadFaces", address_b));
    let elevation = format!("{}.tif", gnosis_name("Elevation", address_a));
    let fixtures = vec![
        Fixture {
            record: profile.resource_metadata_path(&roads).unwrap(),
            metadata: tile_record("RoadNetwork", "Road Network", &roads),
            bytes: vec![0, 159, 146, 150, 255, 1, 2, 3],
            replacement: Some(vec![255, 0, 128, 7, 7, 7]),
            updated: ts(APPLIED_ROADWORKS),
            payload: roads.clone(),
        },
        Fixture {
            record: profile.resource_metadata_path(&faces).unwrap(),
            metadata: topology_record("RoadFaces", &faces),
            bytes: b"faces-v1".to_vec(),
            replacement: None,
            updated: ts(APPLIED_LOAD),
            payload: faces.clone(),
        },
        Fixture {
            record: profile.resource_metadata_path(&elevation).unwrap(),
            metadata: coverage_record("Elevation", &elevation),
            bytes: vec![b'I', b'I', 42, 0, 8, 0, 0, 0],
            replacement: None,
            updated: ts(APPLIED_LOAD),
            payload: elevation.clone(),
        },
    ];

    let tmp = build(&profile, TilingScheme::gnosis_global_grid(), &fixtures);
    let root = tmp.path().join(profile.root_folder_name());
    read_back(&root, &profile, &fixtures);
    assert_fully_conformant(&root, &profile);

    // The 64-bit key pair (§7.12.2) is the address's round-trip: the key
    // read back off the on-disk name rebuilds the address the grid computed.
    for (path, point, address) in [
        (&roads, POINT_A, address_a),
        (&faces, POINT_B, address_b),
        (&elevation, POINT_A, address_a),
    ] {
        let rebuilt = GnosisTileAddress::from_key(parse_gnosis_name(path)).unwrap();
        assert_eq!(rebuilt, address);
        assert_eq!(
            rebuilt,
            GnosisGlobalGrid::tile_at(point.0, point.1, level).unwrap()
        );
        let extent = GnosisGlobalGrid::tile_extent(rebuilt);
        assert!(extent.west <= point.1 && point.1 < extent.east);
        assert!(extent.south <= point.0 && point.0 < extent.north);
    }
}

/// A profile that restricts the core exactly as [`SimulationProfile`] does
/// but **declares only the five mandatory classes** — the case the content
/// sweep exists for.
///
/// Both shipped profiles declare `RequirementsClass::ALL` (deliberately: a
/// profile that pins a tiling scheme and whose datastores legitimately carry
/// attribute models, coverages, topology and journals would otherwise
/// convict its own datastores). Neither can therefore produce a
/// `DeclarationMismatch`, so without a third, deliberately narrow profile the
/// sweep would have no integration-level coverage at all.
struct RestrictedProfile {
    base: SimulationProfile,
}

impl RestrictedProfile {
    fn new() -> Self {
        Self {
            base: SimulationProfile::json(),
        }
    }
}

impl ApplicationProfile for RestrictedProfile {
    fn name(&self) -> &str {
        "restricted"
    }

    fn style_guide(&self) -> StyleGuide {
        self.base.style_guide()
    }

    fn storage_crs(&self) -> Result<StorageCrs, CrsViolation> {
        self.base.storage_crs()
    }

    fn metadata_standard(&self) -> MetadataStandard {
        self.base.metadata_standard()
    }

    fn metadata_encoding(&self) -> MetadataEncoding {
        self.base.metadata_encoding()
    }

    fn uom(&self) -> UnitOfMeasure {
        self.base.uom()
    }

    fn storage_technology(&self) -> StorageTechnology {
        self.base.storage_technology()
    }

    /// The mandatory five and nothing else — Annex A's floor. The datastore
    /// it is pointed at holds attribution, coverage, tiling, topology and
    /// versioning content none of which this profile claims.
    fn conformance_classes(&self) -> Vec<RequirementsClass> {
        RequirementsClass::MANDATORY.to_vec()
    }

    fn is_resource_metadata(&self, logical_path: &str) -> bool {
        self.base.is_resource_metadata(logical_path)
    }

    fn known_extensions(&self) -> Vec<String> {
        self.base.known_extensions()
    }
}

/// Annex A + design spec §4 — the content sweep: content whose requirements
/// class the profile never declared is a
/// [`CdbViolation::DeclarationMismatch`] filed under that class, so a report
/// lists every class *declared* plus every class *betrayed by content*.
///
/// The datastore is byte-identical to the one the simulation pass builds and
/// found fully conformant; only the yardstick changes. That is the point:
/// Annex A judges a datastore against a profile declaration, and the same
/// bytes are conformant or not depending on what was claimed.
#[test]
fn req_core_conformance_restricted_profile_reports_declaration_mismatch() {
    let simulation = SimulationProfile::json();
    let lod = Cdb1Lod::new(3).unwrap();
    let address = Cdb1GlobalGrid::tile_at(POINT_A.0, POINT_A.1, lod).unwrap();
    let roads = format!("{}.gpkg", cdb1_name("RoadNetwork", address));
    let faces = format!("{}.gpkg", cdb1_name("RoadFaces", address));
    let elevation = format!("{}.tif", cdb1_name("Elevation", address));
    let fixtures = vec![
        Fixture {
            record: simulation.resource_metadata_path(&roads).unwrap(),
            metadata: tile_record("RoadNetwork", "Road Network", &roads),
            bytes: b"roads-v1".to_vec(),
            replacement: None,
            updated: ts(APPLIED_LOAD),
            payload: roads.clone(),
        },
        Fixture {
            record: simulation.resource_metadata_path(&faces).unwrap(),
            metadata: topology_record("RoadFaces", &faces),
            bytes: b"faces-v1".to_vec(),
            replacement: None,
            updated: ts(APPLIED_LOAD),
            payload: faces.clone(),
        },
        Fixture {
            record: simulation.resource_metadata_path(&elevation).unwrap(),
            metadata: coverage_record("Elevation", &elevation),
            bytes: b"dem-v1".to_vec(),
            replacement: None,
            updated: ts(APPLIED_LOAD),
            payload: elevation.clone(),
        },
    ];
    let tmp = build(&simulation, TilingScheme::cdb1_global_grid(), &fixtures);
    let root = tmp.path().join(simulation.root_folder_name());

    // Same bytes, declaring profile: conformant.
    assert_fully_conformant(&root, &simulation);

    // Same bytes, restricted profile: five mismatches, one per undeclared
    // class whose content the datastore holds.
    let store = CdbDatastore::open(&root).unwrap();
    let restricted = RestrictedProfile::new();
    let report = store.validate(&restricted).unwrap();
    assert!(!report.is_conformant(), "report: {report}");

    for class in [
        RequirementsClass::Attribution,
        RequirementsClass::Coverages,
        RequirementsClass::Tiling,
        RequirementsClass::Topology,
        RequirementsClass::Versioning,
    ] {
        assert!(
            !report.class_passed(class),
            "{class} content is undeclared and must fail: {report}"
        );
        assert!(
            report.class_has_content(class),
            "{class} content is real; it is the declaration that is missing"
        );
        let violations = report.violations(class);
        assert_eq!(violations.len(), 1, "{class}: {violations:?}");
        match &violations[0] {
            CdbViolation::DeclarationMismatch {
                profile,
                class: filed,
                declared,
                clause,
                ..
            } => {
                assert_eq!(profile, "restricted");
                assert_eq!(*filed, class);
                assert_eq!(
                    declared,
                    &format!("no {} conformance class", class.as_str())
                );
                assert_eq!(*clause, class.requirements_uri());
            }
            other => panic!("{class}: expected DeclarationMismatch, got {other:?}"),
        }
        assert_eq!(violations[0].code(), class.requirements_uri());
    }

    // Geometry is deliberately NOT swept: its content signal (the Geom4
    // `uom`) declares a *unit*, conditional on m coordinates living in a
    // payload the crate does not decode — too weak to convict a profile of
    // failing to declare a class. The record carrying `uom` is present here,
    // so this asserts the asymmetry rather than an absence of content.
    assert!(
        fixtures
            .iter()
            .any(|fixture| fixture.metadata.uom.is_some()),
        "the fixture set must carry a Geom4 uom for this assertion to mean anything"
    );
    assert!(
        report.violations(RequirementsClass::Geometry).is_empty(),
        "Geometry is not swept: {report}"
    );

    // The mandatory five are untouched by the sweep — nothing about the
    // datastore changed, only what was claimed about it.
    for class in RequirementsClass::MANDATORY {
        assert!(report.class_passed(class), "{class}: {report}");
        assert!(report.warnings(class).is_empty(), "{class}: {report}");
    }
}
