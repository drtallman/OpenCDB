//! Integration: `--profile-file` — the JSON descriptor of design §9, and the
//! pre-flight of honesty rule 6 that keeps a broken yardstick from being
//! reported as a datastore defect.
//!
//! §5.1 of the standard makes the application profile the implementable unit,
//! so a tool offering only the two profiles compiled into it serves only the
//! people who already use this crate. The descriptor is what makes cdb-lint an
//! adoption artifact: an implementer states their own profile's restrictions
//! in a file and is judged against them.
//!
//! Two claims are load-bearing here and the rest support them:
//!
//! - **The trait is honoured, not approximated.** A descriptor declaring the
//!   same restrictions as `SimulationProfile` produces the *same report* on
//!   the same datastore as `--profile simulation --encoding json` — byte for
//!   byte, in the frozen wire shape.
//! - **A broken yardstick exits 2.** Every way a descriptor can be wrong is a
//!   usage error, named by descriptor field, decided before the datastore is
//!   opened. Letting `validate` file the profile's own defects as CRS or
//!   Metadata violations would convict the innocent party.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use cdb_lint::profile::DescriptorProfile;
use cdb_lint::{Env, exit, run};
use rusty_cdb::attribution::{AttributeDef, AttributeModel};
use rusty_cdb::coverage::DomainSet;
use rusty_cdb::links::Link;
use rusty_cdb::metadata::{ResourceMetadata, UnitOfMeasure};
use rusty_cdb::profiles::simulation::WGS84_2D_WKT;
use rusty_cdb::tiling::TilingScheme;
use rusty_cdb::topology::WindingOrder;
use rusty_cdb::versioning::PendingCollection;
use rusty_cdb::{ApplicationProfile, CdbDatastore, DatastoreSeed, SimulationProfile};
use serde_json::{Value, json};

/// A terminal-free, colour-free environment: the deterministic one.
fn plain_env() -> Env {
    Env {
        no_color: false,
        stdout_is_terminal: false,
    }
}

/// One run of the whole tool: the exit code, stdout, and stderr.
struct Run {
    code: i32,
    out: String,
    err: String,
}

/// Drive [`run`] with `tokens`.
fn lint(tokens: &[&str]) -> Run {
    let args: Vec<OsString> = tokens.iter().map(OsString::from).collect();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run(&args, &mut out, &mut err, &plain_env());

    Run {
        code,
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
    }
}

/// Drive [`run`] over `root` against the descriptor at `descriptor`.
fn lint_descriptor(descriptor: &Path, root: &Path, extra: &[&str]) -> Run {
    let descriptor = descriptor.to_string_lossy().into_owned();
    let root = root.to_string_lossy().into_owned();
    let mut tokens = vec!["--profile-file", &descriptor];
    tokens.extend_from_slice(extra);
    tokens.push(&root);

    lint(&tokens)
}

/// A seed with the four mandatory §7.9.4.1 identity elements.
fn seed() -> DatastoreSeed {
    DatastoreSeed::new(
        "doi:10.5066/cdb.lint",
        "Lint Fixture",
        "A datastore built for cdb-lint's own tests",
        "ops@example.com",
    )
}

/// A bare datastore built by `profile`, on a fresh temporary directory.
fn datastore(profile: &dyn ApplicationProfile) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    CdbDatastore::create(tmp.path(), profile, seed()).expect("a datastore");
    let root = tmp.path().join(profile.root_folder_name());

    (tmp, root)
}

/// A datastore rich enough that every restriction a descriptor states
/// actually bites.
///
/// A bare datastore is nearly insensitive to the yardstick: with no tiles, no
/// resource records, and no attribute model, a profile can get the tiling
/// scheme and the reserved names wrong and still produce a byte-identical
/// report. Comparing two profiles over one would prove almost nothing, so the
/// mirror test uses this instead — a datastore where all eleven classes carry
/// content, following `tests/full_conformance.rs` in the library and `rich` in
/// `tests/cli_check.rs`.
fn rich(profile: &dyn ApplicationProfile) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let store = CdbDatastore::create(tmp.path(), profile, seed()).expect("a datastore");

    // Tiling8: the scheme rides the global record's conditional element.
    let mut global = store.global_metadata().expect("the global record");
    global.tiling_scheme = Some(TilingScheme::cdb1_global_grid());
    store
        .write_global_metadata(&global)
        .expect("the global record");

    // Attr1-B/C: `global_metadata/vector_attributes.<declared encoding>`.
    store
        .write_attribute_model(&AttributeModel {
            schema_uri: Some("https://example.org/schemas/street.xsd".to_owned()),
            attributes: vec![AttributeDef {
                id: "1".to_owned(),
                name: "StreetName".to_owned(),
                description: "Name of a street as an alphanumeric string".to_owned(),
            }],
        })
        .expect("the attribute model");

    let roads = "/Tiles/RoadNetwork.gpkg";
    let faces = "/Tiles/RoadFaces.gpkg";
    let elevation = "/Tiles/Elevation.tif";

    // Tiling10 makes `keywords` mandatory on every record of a tiled
    // datastore; each record links to the payload it describes (Link1/Link2)
    // and carries one optional class's §7.9.4.2 content signal.
    let mut road_record = ResourceMetadata::new("RoadNetwork", "Road Network", "Vector tile");
    road_record.keywords = vec!["vector".to_owned(), "tile".to_owned()];
    road_record.uom = Some(UnitOfMeasure::Meters);
    road_record.associations = vec![Link::new(roads, "describes").expect("a link")];

    let mut face_record = ResourceMetadata::new("RoadFaces", "Road Faces", "Structured faces");
    face_record.keywords = vec!["topology".to_owned(), "faces".to_owned()];
    face_record.winding_order = Some(WindingOrder::Counterclockwise);
    face_record.associations = vec![Link::new(faces, "describes").expect("a link")];

    let mut dem_record = ResourceMetadata::new("Elevation", "Terrain Elevation", "Gridded DEM");
    dem_record.keywords = vec!["elevation".to_owned(), "terrain".to_owned()];
    dem_record.domain_set = Some(DomainSet::new("m"));
    dem_record.associations = vec![Link::new(elevation, "describes").expect("a link")];

    // The records must exist before a collection may link to them: V3-C
    // refreshes a linked record's `updated`, which presupposes one.
    let mut load = PendingCollection::new().description("initial tile load");
    for (payload, record, bytes) in [
        (roads, road_record, b"roads-v1".to_vec()),
        (faces, face_record, b"faces-v1".to_vec()),
        (elevation, dem_record, b"dem-v1".to_vec()),
    ] {
        let record_path = record_path(profile, payload);
        store
            .write_resource_metadata(&record_path, &record)
            .expect("a resource record");
        load = load.create(payload, bytes).for_record(&record_path);
    }
    store.apply_collection(load).expect("the collection");

    let root = tmp.path().join(profile.root_folder_name());

    (tmp, root)
}

/// The Name5 convention path of a resource's metadata record.
fn record_path(profile: &dyn ApplicationProfile, resource: &str) -> String {
    let (directory, name) = match resource.rfind('/') {
        Some(index) => resource.split_at(index + 1),
        None => ("", resource),
    };
    let stem = name.split('.').next().unwrap_or(name);

    format!(
        "{directory}metadata/{stem}.{}",
        profile.metadata_encoding().extension()
    )
}

/// The descriptor of design §9.1, with only its required fields — the
/// document an implementer writes first.
fn minimal() -> Value {
    json!({
        "name": "acme-sim",
        "case_rule": "PascalCase",
        "language": "en",
        "metadata_standard": "DCAT",
        "metadata_encoding": "json",
        "uom": "M",
        "storage_crs_wkt": WGS84_2D_WKT,
    })
}

/// The descriptor that states exactly what `SimulationProfile::json` states,
/// including its name — so a report taken under it is comparable field for
/// field with one taken under the built-in.
fn mirrors_simulation() -> Value {
    json!({
        "name": "simulation",
        "case_rule": "PascalCase",
        "language": "en",
        "reserved_names": ["metadata"],
        "storage_crs_wkt": WGS84_2D_WKT,
        "metadata_standard": "DCAT",
        "metadata_encoding": "json",
        "uom": "M",
        "storage_technology": "file-system",
        "conformance_classes": "all",
        "tiling_scheme": "CDB1GlobalGrid",
        "resource_metadata_dir": "metadata",
        "root_folder_name": "cdb",
        "known_extensions": ["wkt"],
        "attribute_model": null,
    })
}

/// Write `descriptor` into `dir` as `profile.json` and answer with its path.
fn write_descriptor(dir: &Path, descriptor: &Value) -> PathBuf {
    write_named(
        dir,
        "profile.json",
        &serde_json::to_string_pretty(descriptor).expect("json"),
    )
}

/// Write `contents` into `dir` under `name` and answer with its path.
fn write_named(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    fs::create_dir_all(dir).expect("a directory for the descriptor");
    fs::write(&path, contents).expect("the descriptor");

    path
}

/// The stderr of a run that has to have been refused as a usage error.
fn refused(descriptor: &Value) -> String {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let (_store, root) = datastore(&SimulationProfile::json());
    let path = write_descriptor(tmp.path(), descriptor);

    let run = lint_descriptor(&path, &root, &[]);
    assert_eq!(
        run.code,
        exit::USAGE,
        "the descriptor should have been refused\nstdout:\n{}\nstderr:\n{}",
        run.out,
        run.err
    );
    assert!(
        run.out.is_empty(),
        "a refused yardstick judges nothing, so stdout stays clean:\n{}",
        run.out
    );

    run.err
}

/// Assert that `message` names every one of `expected`.
fn names(message: &str, expected: &[&str]) {
    for fragment in expected {
        assert!(
            message.contains(fragment),
            "the message should name {fragment:?}:\n{message}"
        );
    }
}

// ---------------------------------------------------------------------------
// The descriptor as a working yardstick
// ---------------------------------------------------------------------------

/// The minimal descriptor of design §9.1 produces a profile that judges a
/// real datastore, and the report names it. Everything else here is a way
/// this can fail; this is the feature.
#[test]
fn cli_profile_minimal_descriptor_judges_a_real_datastore() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let path = write_descriptor(tmp.path(), &minimal());
    let (_store, root) = datastore(&SimulationProfile::json());

    let run = lint_descriptor(&path, &root, &[]);

    assert_eq!(
        run.code,
        exit::OK,
        "stdout:\n{}\nstderr:\n{}",
        run.out,
        run.err
    );
    names(&run.out, &["acme-sim (json)", "\nCONFORMANT\n"]);
}

/// The strongest evidence that the descriptor honours `ApplicationProfile`
/// rather than approximating it: a descriptor stating exactly what
/// `SimulationProfile::json` states produces the *same document* on the same
/// datastore as the built-in, in the frozen wire shape, byte for byte.
///
/// A descriptor that merely produced a similar verdict would leave every
/// unasserted field free to differ. This asserts the whole report, over the
/// [`rich`] datastore rather than a bare one — a bare datastore returns the
/// same report whether the profile pins CDB1GlobalGrid or GNOSISGlobalGrid,
/// and whether or not it reserves `metadata`, so the claim would hold
/// vacuously for restrictions the yardstick never got to apply.
#[test]
fn cli_profile_a_descriptor_mirroring_simulation_gives_the_same_report() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let path = write_descriptor(tmp.path(), &mirrors_simulation());
    let (_store, root) = rich(&SimulationProfile::json());
    let root_text = root.to_string_lossy().into_owned();
    let descriptor_text = path.to_string_lossy().into_owned();

    let built_in = lint(&[
        "--profile",
        "simulation",
        "--encoding",
        "json",
        "--format",
        "json",
        &root_text,
    ]);
    let described = lint(&[
        "--profile-file",
        &descriptor_text,
        "--format",
        "json",
        &root_text,
    ]);

    assert_eq!(built_in.code, exit::OK, "{}", built_in.err);
    assert_eq!(described.code, built_in.code, "{}", described.err);
    assert_eq!(
        described.out, built_in.out,
        "the descriptor and the built-in disagree about the same datastore"
    );
}

/// The same claim item by item across the trait, because a report cannot see
/// all of it.
///
/// Two of the descriptor's fields leave no mark on a report even over the
/// [`rich`] datastore: `validate` is handed a root, so it never reads
/// `root_folder_name`, and its tiling cross-check fires only on a *declared*
/// scheme that disagrees with the datastore's, so declaring none is invisible.
/// Both are still part of the yardstick — `CdbDatastore::create` builds the
/// root the first names — and a descriptor that dropped them would be wrong in
/// a way the previous test could not show.
#[test]
fn cli_profile_a_mirroring_descriptor_matches_the_built_in_item_for_item() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let path = write_descriptor(tmp.path(), &mirrors_simulation());
    let described = DescriptorProfile::load(&path).expect("the descriptor should load");
    let built_in = SimulationProfile::json();

    assert_eq!(described.name(), built_in.name());
    assert_eq!(described.metadata_standard(), built_in.metadata_standard());
    assert_eq!(described.metadata_encoding(), built_in.metadata_encoding());
    assert_eq!(described.uom(), built_in.uom());
    assert_eq!(
        described.storage_technology(),
        built_in.storage_technology()
    );
    assert_eq!(
        described.conformance_classes(),
        built_in.conformance_classes()
    );
    assert_eq!(described.root_folder_name(), built_in.root_folder_name());
    assert_eq!(described.tiling_scheme(), built_in.tiling_scheme());
    assert_eq!(described.attribute_model(), built_in.attribute_model());
    assert_eq!(described.known_extensions(), built_in.known_extensions());
    assert_eq!(
        described.language().expect("a language tag"),
        built_in.language().expect("a language tag")
    );

    let (described_crs, built_in_crs) = (
        described.storage_crs().expect("a storage CRS"),
        built_in.storage_crs().expect("a storage CRS"),
    );
    assert_eq!(described_crs.to_wkt(), built_in_crs.to_wkt());

    let (described_guide, built_in_guide) = (described.style_guide(), built_in.style_guide());
    assert_eq!(described_guide.case_rule(), built_in_guide.case_rule());
    assert_eq!(described_guide.language(), built_in_guide.language());
    assert!(described_guide.is_reserved("metadata"));

    for probe in [
        "/Tiles/metadata/Roads.json",
        "/Tiles/Metadata/Roads.JSON",
        "/Tiles/metadata/Roads.gpkg",
        "/Tiles/MetadataRecords/Roads.json",
        "/Tiles/Roads.gpkg",
        "/global_metadata/global_metadata.json",
    ] {
        assert_eq!(
            described.is_resource_metadata(probe),
            built_in.is_resource_metadata(probe),
            "{probe}"
        );
    }
}

/// `conformance_classes` accepts all three of its forms, and the declaration
/// is load-bearing: a profile that declares only the mandatory five over a
/// datastore holding optional-class content convicts it of a
/// `DeclarationMismatch`, which is exactly what the content sweep is for.
#[test]
fn cli_profile_conformance_classes_accepts_all_three_forms() {
    let (_store, root) = datastore(&SimulationProfile::json());

    for form in [
        json!("all"),
        json!("mandatory"),
        json!([
            "attribution",
            "coverages",
            "crs",
            "file-naming",
            "file-structure",
            "geometry",
            "links",
            "metadata",
            "tiling",
            "topology",
            "versioning"
        ]),
    ] {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let mut descriptor = minimal();
        descriptor["conformance_classes"] = form.clone();
        let path = write_descriptor(tmp.path(), &descriptor);

        let run = lint_descriptor(&path, &root, &[]);

        assert_eq!(
            run.code,
            exit::OK,
            "{form} should be accepted\nstdout:\n{}\nstderr:\n{}",
            run.out,
            run.err
        );
    }
}

// ---------------------------------------------------------------------------
// The document: required fields, unknown keys, and the CRS source
// ---------------------------------------------------------------------------

/// Every required field is required, and the diagnostic names the one that is
/// missing. A descriptor that quietly defaulted a required restriction would
/// produce a yardstick nobody stated.
#[test]
fn cli_profile_a_missing_required_field_is_named() {
    for field in [
        "name",
        "case_rule",
        "language",
        "metadata_standard",
        "metadata_encoding",
        "uom",
    ] {
        let mut descriptor = minimal();
        descriptor
            .as_object_mut()
            .expect("an object")
            .remove(field)
            .expect("the field was there to remove");

        names(&refused(&descriptor), &[field]);
    }
}

/// An unknown key is refused and named. A typo that silently fell back to a
/// default would produce the wrong yardstick, which is worse than a failed
/// run: the yardstick is the one thing a conformance tool must not get
/// quietly wrong.
#[test]
fn cli_profile_an_unknown_key_is_refused_and_named() {
    let mut descriptor = minimal();
    descriptor["case_rules"] = json!("PascalCase");

    names(&refused(&descriptor), &["case_rules"]);
}

/// Exactly one storage-CRS source. Neither leaves the profile without the one
/// CRS Requirement CRS3 makes it declare; both leave two, with no rule for
/// which wins.
#[test]
fn cli_profile_exactly_one_storage_crs_source() {
    let mut neither = minimal();
    neither
        .as_object_mut()
        .expect("an object")
        .remove("storage_crs_wkt");
    names(
        &refused(&neither),
        &["storage_crs_wkt", "storage_crs_wkt_path"],
    );

    let mut both = minimal();
    both["storage_crs_wkt_path"] = json!("crs.wkt");
    names(
        &refused(&both),
        &["storage_crs_wkt", "storage_crs_wkt_path"],
    );
}

/// `storage_crs_wkt_path` resolves against the descriptor's own directory,
/// never the working directory. The descriptor names a bare `crs.wkt` that
/// exists only in the subdirectory holding the descriptor — the process's
/// working directory is the crate root, which has no such file — so a run
/// that succeeds can only have resolved the path where the descriptor lives.
///
/// A descriptor is a document a team checks in and shares. Resolving its
/// references against whatever directory the operator happened to be standing
/// in would make it mean different things in different shells.
#[test]
fn cli_profile_a_crs_path_resolves_against_the_descriptor() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let beside = tmp.path().join("conf");
    write_named(&beside, "crs.wkt", WGS84_2D_WKT);

    let mut descriptor = minimal();
    descriptor
        .as_object_mut()
        .expect("an object")
        .remove("storage_crs_wkt");
    descriptor["storage_crs_wkt_path"] = json!("crs.wkt");
    let path = write_descriptor(&beside, &descriptor);

    assert!(
        !Path::new("crs.wkt").exists(),
        "the working directory must not hold a `crs.wkt`, or this proves nothing"
    );

    let (_store, root) = datastore(&SimulationProfile::json());
    let run = lint_descriptor(&path, &root, &[]);

    assert_eq!(
        run.code,
        exit::OK,
        "stdout:\n{}\nstderr:\n{}",
        run.out,
        run.err
    );
}

/// A `storage_crs_wkt_path` that names nothing is a usage error naming both
/// the field and the path it resolved to — the resolved path, because the
/// whole point of the previous test is that the written path is not the one
/// that was opened.
#[test]
fn cli_profile_an_unreadable_crs_path_is_refused() {
    let mut descriptor = minimal();
    descriptor
        .as_object_mut()
        .expect("an object")
        .remove("storage_crs_wkt");
    descriptor["storage_crs_wkt_path"] = json!("nowhere.wkt");

    names(
        &refused(&descriptor),
        &["storage_crs_wkt_path", "nowhere.wkt"],
    );
}

/// A descriptor that is not JSON at all, and a descriptor that is not there,
/// are both usage errors: nothing was judged, and the request was the problem.
#[test]
fn cli_profile_an_unreadable_descriptor_is_a_usage_error() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let (_store, root) = datastore(&SimulationProfile::json());

    let missing = tmp.path().join("absent.json");
    let run = lint_descriptor(&missing, &root, &[]);
    assert_eq!(run.code, exit::USAGE, "{}", run.err);
    names(&run.err, &["absent.json"]);

    let malformed = write_named(tmp.path(), "malformed.json", "{ this is not json");
    let run = lint_descriptor(&malformed, &root, &[]);
    assert_eq!(run.code, exit::USAGE, "{}", run.err);
    names(&run.err, &["malformed.json"]);
}

// ---------------------------------------------------------------------------
// The vocabulary: the crate's, and its rejections say so
// ---------------------------------------------------------------------------

/// A value outside a closed vocabulary is refused, and the message lists the
/// spellings that would have worked. An error that says only "invalid" makes
/// the user go and find what the tool already knew.
#[test]
fn cli_profile_a_rejected_value_lists_the_valid_spellings() {
    for (field, bad, expected) in [
        (
            "case_rule",
            json!("PASCALCASE"),
            &["PascalCase", "kebab-case"][..],
        ),
        (
            "metadata_standard",
            json!("dcat"),
            &["DCAT", "ISO-19115:2019"][..],
        ),
        ("metadata_encoding", json!("JSON"), &["json", "xml"][..]),
        ("uom", json!("metres"), &["M", "FT"][..]),
        (
            "tiling_scheme",
            json!("cdb1"),
            &["CDB1GlobalGrid", "GNOSISGlobalGrid"][..],
        ),
        ("storage_technology", json!("s3"), &["file-system"][..]),
    ] {
        let mut descriptor = minimal();
        descriptor[field] = bad.clone();

        let message = refused(&descriptor);
        names(&message, &[field]);
        names(&message, expected);
    }
}

/// An unknown class token is refused and the class vocabulary is listed. A
/// declaration is what decides which stages run, so a misspelled class would
/// silently narrow the audit.
#[test]
fn cli_profile_an_unknown_class_token_is_refused() {
    let mut descriptor = minimal();
    descriptor["conformance_classes"] = json!(["crs", "file-nameing"]);

    let message = refused(&descriptor);
    names(
        &message,
        &["conformance_classes", "file-nameing", "file-naming"],
    );
}

/// `conformance_classes` is a keyword or a list of tokens, and nothing else;
/// the refusal names all three forms.
#[test]
fn cli_profile_a_malformed_conformance_classes_is_refused() {
    let mut descriptor = minimal();
    descriptor["conformance_classes"] = json!(7);

    names(
        &refused(&descriptor),
        &["conformance_classes", "all", "mandatory"],
    );
}

// ---------------------------------------------------------------------------
// Pre-flight — design §5 rule 6: a broken yardstick is a usage error
// ---------------------------------------------------------------------------

/// Honesty rule 6, the CRS half. `ApplicationProfile::storage_crs` returns a
/// `Result`, and a descriptor whose WKT does not parse makes it fail. Called
/// during validation instead, that failure would be filed as a CRS violation
/// against a datastore that did nothing wrong — so it is a usage error, exit
/// 2, decided before the datastore is opened.
#[test]
fn req_profile_preflight_malformed_wkt_is_a_usage_error() {
    let mut descriptor = minimal();
    descriptor["storage_crs_wkt"] = json!("GEOGCRS[\"unterminated\"");

    let message = refused(&descriptor);
    names(&message, &["storage_crs_wkt"]);
    assert!(
        !message.contains("NON-CONFORMANT"),
        "the datastore is not the party at fault:\n{message}"
    );
}

/// Honesty rule 6, the language half. `ApplicationProfile::language` returns a
/// `Result` and derives from the style guide, so a malformed BCP 47 tag in the
/// descriptor is a defect of the yardstick, not of the datastore.
#[test]
fn req_profile_preflight_a_bad_language_tag_is_a_usage_error() {
    let mut descriptor = minimal();
    descriptor["language"] = json!("en_US!");

    names(&refused(&descriptor), &["language", "en_US!"]);
}

/// Honesty rule 6, the attribute-model half. A declared model is validated by
/// the library's own document check before the datastore is opened, so an
/// invalid one is a usage error rather than an Attribution finding against
/// whatever the datastore happens to hold.
#[test]
fn req_profile_preflight_an_invalid_attribute_model_is_a_usage_error() {
    let mut descriptor = minimal();
    descriptor["attribute_model"] = json!({
        "attributes": [
            {"id": "1", "name": "StreetName", "description": "Name of a street"},
            {"id": "1", "name": "StreetType", "description": "Type of a street"},
        ]
    });

    names(&refused(&descriptor), &["attribute_model"]);
}

/// `gpkg` parses as a metadata encoding and this build implements none, so
/// the descriptor — the only door that can reach the state — closes it at
/// pre-flight with exit 2 rather than letting `GlobalMetadata::write_to`
/// surface as an operational failure.
///
/// Exit 3 means "the tool could not inspect your datastore". The truth here is
/// "you asked for an encoding this build does not implement", which is a fact
/// about the request, and the message says the stance is deliberate.
#[test]
fn req_profile_preflight_rejects_gpkg_as_a_deliberate_stance() {
    let mut descriptor = minimal();
    descriptor["metadata_encoding"] = json!("gpkg");

    let message = refused(&descriptor);
    names(&message, &["metadata_encoding", "gpkg", "deliberate"]);
}

/// A name-shaped field whose value can never match one path component is a
/// dead yardstick, and a dead yardstick exits 2 — never a report that
/// quietly checked less than it claims.
///
/// The load-bearing case is `resource_metadata_dir`: a value carrying a
/// path separator recognizes no record at all, so the Metadata and Links
/// stages read nothing, and a datastore whose only violations live inside
/// its records prints CONFORMANT. That is the false green the whole tool
/// exists to prevent, reached through one trailing character in a
/// descriptor (final review, 2026-09-14).
#[test]
fn req_profile_preflight_refuses_a_component_that_cannot_match() {
    for (field, value) in [
        ("resource_metadata_dir", json!("metadata/")),
        ("resource_metadata_dir", json!("")),
        ("resource_metadata_dir", json!("a/b")),
        ("root_folder_name", json!("")),
        ("root_folder_name", json!("srv/cdb")),
    ] {
        let mut descriptor = minimal();
        descriptor[field] = value.clone();
        let message = refused(&descriptor);
        names(&message, &[field]);
        assert!(
            message.contains("component") || message.contains("match"),
            "{field}={value}: the refusal says why the value is dead: {message}"
        );
    }
}

/// The profile's `name` is the report's `profile` field, the header line,
/// and a baseline's identity. A blank name says nothing there, and a
/// control character forges report structure — a newline yields a second
/// `profile` header line a reader has no way to distrust.
#[test]
fn req_profile_preflight_refuses_an_unusable_name() {
    for value in ["", "   ", "acme\nprofile    forged (xml)", "acme\rcr"] {
        let mut descriptor = minimal();
        descriptor["name"] = json!(value);
        names(&refused(&descriptor), &["name"]);
    }
}

/// A voucher entry that can never match vouches for nothing, and the author
/// deserves to hear it at exit 2 rather than watch `--deny-warnings` fail a
/// build on warnings they explicitly vouched away. The dotted diagnostic
/// teaches the spelling: an extension is written without its dot.
#[test]
fn req_profile_preflight_refuses_inert_voucher_entries() {
    for (field, value) in [
        ("known_extensions", json!([""])),
        ("known_extensions", json!(["a/b"])),
        ("reserved_names", json!([""])),
        ("reserved_names", json!(["a/b"])),
    ] {
        let mut descriptor = minimal();
        descriptor[field] = value.clone();
        names(&refused(&descriptor), &[field]);
    }

    let mut descriptor = minimal();
    descriptor["known_extensions"] = json!([".tif"]);
    names(
        &refused(&descriptor),
        &["known_extensions", "without its dot"],
    );
}

/// The declared convention directory is exempt from the case rule
/// automatically, exactly as `SimulationProfile` reserves its own
/// `metadata/` (a Requirement Name5 duty): a minimal descriptor must not
/// convict the very layout its own `resource_metadata_dir` default
/// declares. Before this held, `minimal()` over a datastore the crate
/// itself wrote filed a CaseRuleViolation for every lowercase `metadata`
/// component (final review, 2026-09-14).
#[test]
fn cli_profile_the_convention_directory_is_reserved_automatically() {
    let (_store, root) = rich(&SimulationProfile::json());

    // The rich store declares CDB1GlobalGrid and holds a .wkt CRS record, so
    // the descriptor states both — but deliberately never spells
    // `reserved_names`.
    let mut descriptor = minimal();
    descriptor["tiling_scheme"] = json!("CDB1GlobalGrid");
    descriptor["known_extensions"] = json!(["wkt"]);
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let path = write_descriptor(tmp.path(), &descriptor);

    let run = lint_descriptor(&path, &root, &[]);

    assert_eq!(
        run.code,
        exit::OK,
        "the crate's own layout conforms under a minimal descriptor\nstdout:\n{}\nstderr:\n{}",
        run.out,
        run.err
    );
    assert!(
        !run.out.contains("name-case"),
        "no case finding on the convention directory:\n{}",
        run.out
    );
}

// ---------------------------------------------------------------------------
// One yardstick per run
// ---------------------------------------------------------------------------

/// `--profile-file` excludes the built-in flags. The grammar already enforces
/// it; this holds the rule now that the descriptor arm actually resolves,
/// because a run with two yardsticks would have an unattributable verdict.
#[test]
fn cli_profile_a_descriptor_cannot_be_combined_with_a_built_in() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let path = write_descriptor(tmp.path(), &minimal());
    let (_store, root) = datastore(&SimulationProfile::json());
    let descriptor = path.to_string_lossy().into_owned();
    let root = root.to_string_lossy().into_owned();

    for extra in [
        vec!["--profile", "simulation"],
        vec!["--encoding", "json"],
        vec!["--profile", "simulation", "--encoding", "json"],
    ] {
        let mut tokens = vec!["--profile-file", &descriptor];
        tokens.extend_from_slice(&extra);
        tokens.push(&root);

        let run = lint(&tokens);

        assert_eq!(run.code, exit::USAGE, "{extra:?}:\n{}", run.err);
        names(&run.err, &["--profile-file"]);
    }
}
