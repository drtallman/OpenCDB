//! Integration: the `check` path — open a real datastore, judge it against a
//! built-in profile, render the Annex-A-style text report of design §6.1, and
//! exit with the code CI reads.
//!
//! **Every fixture is a real datastore.** `CdbDatastore::create` builds it on
//! a `tempfile` directory and the library judges it; nothing here mocks a
//! report. That is not fastidiousness: the honesty rules of design §5 are
//! claims about what a *datastore* makes the library say, and a hand-built
//! report could be made to say anything, so a test over one would prove
//! nothing about the tool. The three-valued coverage the rules turn on
//! (`checked` / `none` / `unchecked`) only arises from content the crate
//! deliberately does not decode — `tests/full_conformance.rs` in the library
//! builds the same shape, and [`rich`] follows it.
//!
//! No test spawns a process: [`run`] takes its arguments as a slice, its
//! ambient state as an [`Env`], and writes to two sinks, so the whole tool is
//! driven in-process and no result depends on whether the harness happened to
//! be attached to a terminal.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use cdb_lint::render::text::{TextOptions, render};
use cdb_lint::{Env, exit, run};
use rusty_cdb::attribution::{AttributeDef, AttributeModel};
use rusty_cdb::conformance::{ContentCoverage, RequirementsClass};
use rusty_cdb::coverage::DomainSet;
use rusty_cdb::crs::{CrsViolation, StorageCrs};
use rusty_cdb::links::Link;
use rusty_cdb::metadata::{MetadataEncoding, MetadataStandard, ResourceMetadata, UnitOfMeasure};
use rusty_cdb::naming::StyleGuide;
use rusty_cdb::profiles::{ApplicationProfile, StorageTechnology, TilingSchemeId};
use rusty_cdb::tiling::TilingScheme;
use rusty_cdb::topology::WindingOrder;
use rusty_cdb::versioning::PendingCollection;
use rusty_cdb::{CdbDatastore, DatastoreSeed, GnosisProfile, SimulationProfile};

/// The ANSI sequences design §6.1 pins to the four status tokens. Written out
/// here rather than imported so the test would notice the renderer silently
/// agreeing with itself about a changed escape.
const GREEN: &str = "\u{1b}[32m";
const RED: &str = "\u{1b}[31m";
const YELLOW: &str = "\u{1b}[33m";
const DIM: &str = "\u{1b}[2m";
const RESET: &str = "\u{1b}[0m";

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

impl Run {
    /// Assert that stdout names every one of `expected`.
    fn out_names(&self, expected: &[&str]) {
        for fragment in expected {
            assert!(
                self.out.contains(fragment),
                "stdout should contain {fragment:?}:\n{}",
                self.out
            );
        }
    }
}

/// Drive [`run`] with `tokens`, the last of which is usually a root.
fn lint(tokens: &[&str], env: &Env) -> Run {
    let args: Vec<OsString> = tokens.iter().map(OsString::from).collect();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run(&args, &mut out, &mut err, env);

    Run {
        code,
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
    }
}

/// Drive [`run`] over `root` under the `simulation`/`json` yardstick.
fn lint_sim(root: &Path, extra: &[&str]) -> Run {
    let mut tokens = vec!["--profile", "simulation", "--encoding", "json"];
    tokens.extend_from_slice(extra);
    let root = root.to_string_lossy().into_owned();
    tokens.push(&root);

    lint(&tokens, &plain_env())
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

/// A bare datastore: the five mandatory classes carry content, and every
/// optional class is declared, listed, and empty.
fn bare(profile: &dyn ApplicationProfile) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    CdbDatastore::create(tmp.path(), profile, seed()).unwrap();
    let root = tmp.path().join(profile.root_folder_name());

    (tmp, root)
}

/// The §7.1.2.3 example attribute model (Requirement Attr2).
fn street_model() -> AttributeModel {
    AttributeModel {
        schema_uri: Some("https://example.org/schemas/street.xsd".to_owned()),
        attributes: vec![AttributeDef {
            id: "1".to_owned(),
            name: "StreetName".to_owned(),
            description: "Name of a street as an alphanumeric string".to_owned(),
        }],
    }
}

/// A datastore rich enough that all eleven classes carry content and the
/// coverage vocabulary is genuinely three-valued.
///
/// The shape follows `tests/full_conformance.rs` in the library: a
/// `tilingScheme` on the global record (Tiling8), an attribute model
/// (Attr1-B/C), and three resource records carrying the three §7.9.4.2
/// conditional elements that are the optional classes' content signals — the
/// Geom4 `uom`, the Coverages6 `domainSet`, and the Face4 `windingOrder` —
/// plus a versioning collection that creates the payloads they describe.
///
/// Geometry and Topology come out `unchecked` rather than `checked`, and that
/// asymmetry is the point: their subjects live inside payloads this crate
/// deliberately does not decode (`docs/CONFORMANCE.md` §6), so no fixture can
/// make them `checked`, and a linter painting them green would tell the exact
/// lie the third state exists to prevent.
fn rich(profile: &dyn ApplicationProfile, scheme: TilingScheme) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let store = CdbDatastore::create(tmp.path(), profile, seed()).unwrap();

    // Tiling8: the scheme rides the global record's conditional element.
    let mut global = store.global_metadata().unwrap();
    global.tiling_scheme = Some(scheme);
    store.write_global_metadata(&global).unwrap();

    // Attr1-B/C: `global_metadata/vector_attributes.<declared encoding>`.
    store.write_attribute_model(&street_model()).unwrap();

    let roads = "/Tiles/RoadNetwork.gpkg";
    let faces = "/Tiles/RoadFaces.gpkg";
    let elevation = "/Tiles/Elevation.tif";

    // Tiling10 makes `keywords` mandatory on every record of a tiled
    // datastore; each record also links to the payload it describes
    // (Link1/Link2) and carries one optional class's content signal.
    let mut road_record = ResourceMetadata::new("RoadNetwork", "Road Network", "Vector tile");
    road_record.keywords = vec!["vector".to_owned(), "tile".to_owned()];
    road_record.uom = Some(UnitOfMeasure::Meters);
    road_record.associations = vec![Link::new(roads, "describes").unwrap()];

    let mut face_record = ResourceMetadata::new("RoadFaces", "Road Faces", "Structured faces");
    face_record.keywords = vec!["topology".to_owned(), "faces".to_owned()];
    face_record.winding_order = Some(WindingOrder::Counterclockwise);
    face_record.associations = vec![Link::new(faces, "describes").unwrap()];

    let mut dem_record = ResourceMetadata::new("Elevation", "Terrain Elevation", "Gridded DEM");
    dem_record.keywords = vec!["elevation".to_owned(), "terrain".to_owned()];
    dem_record.domain_set = Some(DomainSet::new("m"));
    dem_record.associations = vec![Link::new(elevation, "describes").unwrap()];

    let payloads = [
        (roads, road_record, b"roads-v1".to_vec()),
        (faces, face_record, b"faces-v1".to_vec()),
        (elevation, dem_record, b"dem-v1".to_vec()),
    ];

    // The records must exist before a collection may link to them: V3-C
    // refreshes a linked record's `updated`, which presupposes one.
    let mut load = PendingCollection::new().description("initial tile load");
    for (payload, record, bytes) in payloads {
        let record_path = record_path(profile, payload);
        store
            .write_resource_metadata(&record_path, &record)
            .unwrap();
        load = load.create(payload, bytes).for_record(&record_path);
    }
    store.apply_collection(load).unwrap();

    let root = tmp.path().join(profile.root_folder_name());

    (tmp, root)
}

/// The Name5 convention path of a resource's metadata record, asked of
/// whichever built-in profile is in play.
fn record_path(profile: &dyn ApplicationProfile, resource: &str) -> String {
    let (directory, name) = match resource.rfind('/') {
        Some(index) => resource.split_at(index + 1),
        None => ("", resource),
    };
    let stem = name.split('.').next().unwrap();

    format!(
        "{directory}metadata/{stem}.{}",
        profile.metadata_encoding().extension()
    )
}

// ---------------------------------------------------------------------------
// The verdict, the rows, and the exit codes
// ---------------------------------------------------------------------------

/// A datastore that conforms: every class passes, the verdict says so, and
/// the exit code is 0 — the answer a CI job actually branches on.
#[test]
fn cli_check_conformant_datastore_passes_every_class() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = rich(&profile, TilingScheme::cdb1_global_grid());

    let run = lint_sim(&root, &[]);

    assert_eq!(
        run.code,
        exit::OK,
        "stdout:\n{}\nstderr:\n{}",
        run.out,
        run.err
    );
    assert!(run.out.contains("\nCONFORMANT\n"), "{}", run.out);
    assert!(!run.out.contains("NON-CONFORMANT"), "{}", run.out);
    for &class in RequirementsClass::ALL {
        assert!(
            run.out.contains(class.as_str()),
            "{class} is missing from the report:\n{}",
            run.out
        );
    }
    assert!(!run.out.contains("[FAIL]"), "{}", run.out);
}

/// The header names both crates, the datastore, and the yardstick. A report
/// that does not say what it was measured against is not auditable.
#[test]
fn cli_check_header_names_the_tool_the_datastore_and_the_yardstick() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = bare(&profile);

    let run = lint_sim(&root, &[]);

    run.out_names(&[
        &format!(
            "cdb-lint {} (rusty_cdb {})\n",
            env!("CARGO_PKG_VERSION"),
            cdb_lint::RUSTY_CDB_VERSION
        ),
        &root.display().to_string(),
        "simulation (json)",
    ]);
}

/// The whole report, byte for byte, against the layout design §6.1 draws.
///
/// The other tests here read the report by fragment, which is what keeps them
/// legible; this one pins the geometry none of them can see — the column the
/// class names start in, the column the coverage notes line up on, the blank
/// lines around the rows, and the order of the eleven classes. Any of those
/// can drift without a single fragment assertion noticing, and a report whose
/// columns wander is a report nobody can diff between two runs.
///
/// A bare datastore is the fixture because its output is a pure function of
/// the code: five mandatory classes with content, six optional classes
/// governing nothing, no findings, and one path.
#[test]
fn cli_check_the_report_reproduces_the_layout_of_design_6_1() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = bare(&profile);

    let run = lint_sim(&root, &[]);

    let expected = format!(
        "cdb-lint {} (rusty_cdb {})\n\
         datastore  {}\n\
         profile    simulation (json)\n\
         \n\
         \x20 [N/A]       attribution        no content this class governs\n\
         \x20 [N/A]       coverages          no content this class governs\n\
         \x20 [PASS]      crs\n\
         \x20 [PASS]      file-naming\n\
         \x20 [PASS]      file-structure\n\
         \x20 [N/A]       geometry           no content this class governs\n\
         \x20 [PASS]      links\n\
         \x20 [PASS]      metadata\n\
         \x20 [N/A]       tiling             no content this class governs\n\
         \x20 [N/A]       topology           no content this class governs\n\
         \x20 [N/A]       versioning         no content this class governs\n\
         \n\
         11 classes: 5 checked, 6 no content, 0 not checked · 0 violations · 0 warnings\n\
         CONFORMANT\n",
        env!("CARGO_PKG_VERSION"),
        cdb_lint::RUSTY_CDB_VERSION,
        root.display(),
    );

    assert_eq!(run.out, expected);
    assert_eq!(run.code, exit::OK);
    assert!(run.err.is_empty(), "{}", run.err);
}

/// A violation fails its class, the verdict, and the build. Design §4.1 keeps
/// this apart from exit 3: the datastore was inspected, and it did not
/// conform.
#[test]
fn cli_check_a_violation_fails_its_class_and_the_build() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = bare(&profile);
    // Requirement Name1 (`/req/core/name-spaces`): no whitespace in a name.
    fs::create_dir(root.join("My Tiles")).unwrap();
    fs::write(root.join("My Tiles").join("Payload.gpkg"), b"x").unwrap();

    let run = lint_sim(&root, &[]);

    assert_eq!(run.code, exit::FINDINGS, "{}", run.out);
    assert!(run.out.contains("[FAIL]"), "{}", run.out);
    assert!(run.out.contains("file-naming"), "{}", run.out);
    assert!(run.out.contains("/req/core/name-spaces"), "{}", run.out);
    assert!(
        run.out.trim_end().ends_with("NON-CONFORMANT"),
        "{}",
        run.out
    );
}

/// A finding prints the library's own message verbatim beside its stable
/// code. cdb-lint rewrites nothing: the message is the crate's, and the code
/// is what `cdb-lint explain` will take.
#[test]
fn cli_check_findings_print_the_code_beside_the_crate_s_own_message() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = bare(&profile);
    fs::create_dir(root.join("My Tiles")).unwrap();

    let store = CdbDatastore::open(&root).unwrap();
    let report = store.validate(&profile).unwrap();
    let expected: Vec<String> = report
        .violations(RequirementsClass::FileNaming)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(!expected.is_empty(), "the fixture must produce a violation");

    let run = lint_sim(&root, &[]);

    for message in &expected {
        assert!(
            run.out.contains(message.as_str()),
            "the crate's own message must appear verbatim: {message:?}\n{}",
            run.out
        );
    }
}

/// Warnings are SHOULD findings, and a SHOULD does not decide conformance:
/// the datastore conforms and the build passes. `--deny-warnings` moves the
/// exit code without moving the verdict, and the verdict line names the flag
/// responsible (design §5 rule 4).
#[test]
fn cli_check_warnings_conform_until_deny_warnings_is_asked_for() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = bare(&profile);
    // Recommendation `/req/core/name-empty-folders-A`: an empty folder is a
    // SHOULD finding.
    fs::create_dir(root.join("Tiles")).unwrap();

    let quiet_run = lint_sim(&root, &[]);
    assert_eq!(quiet_run.code, exit::OK, "{}", quiet_run.out);
    assert!(
        quiet_run.out.contains("\nCONFORMANT\n"),
        "{}",
        quiet_run.out
    );
    assert!(quiet_run.out.contains("1 warning"), "{}", quiet_run.out);

    let denied = lint_sim(&root, &["--deny-warnings"]);
    assert_eq!(denied.code, exit::FINDINGS, "{}", denied.out);
    assert!(
        denied
            .out
            .contains("CONFORMANT (1 warning) — exit 1 by --deny-warnings"),
        "{}",
        denied.out
    );
    assert!(!denied.out.contains("NON-CONFORMANT"), "{}", denied.out);
}

/// `--deny-warnings` over a datastore with no warnings changes nothing: the
/// flag attributes the exit code only when it actually moved it.
#[test]
fn cli_check_deny_warnings_is_silent_when_there_are_no_warnings() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = bare(&profile);

    let run = lint_sim(&root, &["--deny-warnings"]);

    assert_eq!(run.code, exit::OK, "{}", run.out);
    assert!(run.out.contains("\nCONFORMANT\n"), "{}", run.out);
    assert!(!run.out.contains("--deny-warnings"), "{}", run.out);
}

/// Both built-in profiles resolve and judge. The trait's premise is that
/// profiles vary (§5.1), and only a second profile tests it.
#[test]
fn cli_check_both_builtin_profiles_resolve_and_validate() {
    for (name, profile, scheme) in [
        (
            "simulation",
            Box::new(SimulationProfile::xml()) as Box<dyn ApplicationProfile>,
            TilingScheme::cdb1_global_grid(),
        ),
        (
            "gnosis",
            Box::new(GnosisProfile::xml()),
            TilingScheme::gnosis_global_grid(),
        ),
    ] {
        let (_tmp, root) = rich(&*profile, scheme);
        let root = root.to_string_lossy().into_owned();

        let run = lint(
            &["--profile", name, "--encoding", "xml", &root],
            &plain_env(),
        );

        assert_eq!(run.code, exit::OK, "{name}:\n{}\n{}", run.out, run.err);
        assert!(run.out.contains(&format!("{name} (xml)")), "{}", run.out);
        assert!(run.out.contains("\nCONFORMANT\n"), "{name}:\n{}", run.out);
    }
}

/// Each profile pins its own tiling scheme, so pointing one at the other's
/// datastore is a real disagreement — and the report says so rather than
/// quietly adopting whatever the datastore declares.
#[test]
fn cli_check_a_profile_judges_a_datastore_built_for_the_other_one() {
    let (_tmp, root) = rich(&GnosisProfile::json(), TilingScheme::gnosis_global_grid());

    let run = lint_sim(&root, &[]);

    assert_eq!(run.code, exit::FINDINGS, "{}\n{}", run.out, run.err);
    assert!(
        run.out.contains("/req/core/tiling-tilingscheme-consistent"),
        "{}",
        run.out
    );
}

// ---------------------------------------------------------------------------
// Coverage: the honesty rules of design §5
// ---------------------------------------------------------------------------

/// Honesty rule 1 (`docs/CONFORMANCE.md` §6): for Geometry and Topology a
/// pass means "we did not look", never "we looked and it was fine". The row
/// reads `UNCHECKED`, carries the note saying why, and is **never** the token
/// or the colour of a clean pass — not under `--color always`, not under any
/// flag. A linter painting those green would tell the exact lie the third
/// coverage state was introduced to prevent.
#[test]
fn req_cdb_lint_unchecked_never_renders_as_pass() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = rich(&profile, TilingScheme::cdb1_global_grid());

    // The fixture has to genuinely produce the third state, or the assertions
    // below would pass over a report that never exercised them.
    let store = CdbDatastore::open(&root).unwrap();
    let report = store.validate(&profile).unwrap();
    for class in [RequirementsClass::Geometry, RequirementsClass::Topology] {
        assert_eq!(
            report.class_coverage(class),
            ContentCoverage::Unchecked,
            "the fixture must put {class} into the third state"
        );
    }

    let run = lint_sim(&root, &[]);
    for class in ["geometry", "topology"] {
        let row = row_for(&run.out, class);
        assert!(row.contains("[UNCHECKED]"), "{row:?}");
        assert!(!row.contains("[PASS]"), "{row:?}");
        assert!(
            row.contains("content present, no datastore-level check"),
            "{row:?}"
        );
    }

    let coloured = lint(
        &[
            "--profile",
            "simulation",
            "--encoding",
            "json",
            "--color",
            "always",
            &root.to_string_lossy(),
        ],
        &plain_env(),
    );
    assert!(
        coloured
            .out
            .contains(&format!("{YELLOW}[UNCHECKED]{RESET}")),
        "UNCHECKED is yellow:\n{}",
        coloured.out
    );
    assert!(
        !coloured.out.contains(&format!("{GREEN}[UNCHECKED]")),
        "UNCHECKED must never be green:\n{}",
        coloured.out
    );
}

/// Honesty rule 2: no output omits coverage. The aggregate tally is the one
/// line that always carries it, and `--quiet` never suppresses it — a reader
/// who sees only the summary still learns how much was actually checked.
#[test]
fn req_cdb_lint_the_coverage_tally_survives_quiet() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = rich(&profile, TilingScheme::cdb1_global_grid());

    let loud = lint_sim(&root, &[]);
    let quiet = lint_sim(&root, &["--quiet"]);

    let tally = "11 classes: 9 checked, 0 no content, 2 not checked · 0 violations · 0 warnings";
    for run in [&loud, &quiet] {
        assert!(run.out.contains(tally), "{}", run.out);
        assert!(run.out.contains("\nCONFORMANT\n"), "{}", run.out);
    }
    // Quiet drops the passing rows and nothing else.
    assert!(loud.out.contains("[PASS]"), "{}", loud.out);
    assert!(!quiet.out.contains("[PASS]"), "{}", quiet.out);
}

/// The three counts are counts, not decoration: a bare datastore has six
/// optional classes governing nothing, and the tally says six.
#[test]
fn req_cdb_lint_the_tally_counts_all_three_coverage_states() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = bare(&profile);

    let run = lint_sim(&root, &[]);

    assert!(
        run.out.contains(
            "11 classes: 5 checked, 6 no content, 0 not checked · 0 violations · 0 warnings"
        ),
        "{}",
        run.out
    );
}

/// `--quiet` suppresses passing rows, never a failing one, its findings, the
/// tally, or the verdict.
#[test]
fn cli_check_quiet_keeps_the_failures_the_tally_and_the_verdict() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = bare(&profile);
    fs::create_dir(root.join("My Tiles")).unwrap();
    fs::write(root.join("My Tiles").join("Payload.gpkg"), b"x").unwrap();

    let run = lint_sim(&root, &["--quiet"]);

    assert_eq!(run.code, exit::FINDINGS, "{}", run.out);
    assert!(run.out.contains("[FAIL]"), "{}", run.out);
    assert!(run.out.contains("/req/core/name-spaces"), "{}", run.out);
    assert!(run.out.contains("11 classes:"), "{}", run.out);
    assert!(run.out.contains("NON-CONFORMANT"), "{}", run.out);
    assert!(!run.out.contains("[PASS]"), "{}", run.out);
}

/// A class carrying a warning survives `--quiet` too.
///
/// Design §6.1 says `--quiet` suppresses *passing* rows, and a
/// warning-carrying class technically passes — a SHOULD does not decide
/// conformance. Suppressing it anyway would put `--quiet --deny-warnings` in
/// the absurd position of failing the build over warnings it refused to
/// print. So the rule cdb-lint implements is the one that reading was
/// reaching for: `--quiet` drops the rows with **nothing to report**.
#[test]
fn cli_check_quiet_keeps_a_class_that_has_a_warning_to_report() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = bare(&profile);
    fs::create_dir(root.join("Tiles")).unwrap();

    let run = lint_sim(&root, &["--quiet", "--deny-warnings"]);

    assert_eq!(run.code, exit::FINDINGS, "{}", run.out);
    assert!(run.out.contains("file-structure"), "{}", run.out);
    assert!(
        run.out.contains("/req/core/name-empty-folders-A"),
        "the warning that failed the build has to be visible:\n{}",
        run.out
    );
    assert!(
        run.out
            .contains("CONFORMANT (1 warning) — exit 1 by --deny-warnings"),
        "{}",
        run.out
    );
    // Every other class still goes quiet.
    assert!(!run.out.contains("attribution"), "{}", run.out);
}

/// A profile declaring **less** than its datastore holds is convicted by the
/// content sweep, and the classes it convicts report `unchecked` — no stage
/// ran over that content (`docs/CONFORMANCE.md` §6). Status and coverage are
/// therefore orthogonal, and the row has to print both: dropping the note on
/// a `FAIL` row would report a datastore as *more* checked the less its
/// profile declared.
///
/// Neither built-in profile can reach this state — both declare every class,
/// deliberately — so the renderer is driven directly here rather than through
/// [`run`]. That is the only way the case exists at all, and it is a real
/// report from a real datastore, not a hand-built one.
#[test]
fn req_cdb_lint_a_failing_row_still_prints_its_coverage_note() {
    let (_tmp, root) = rich(&SimulationProfile::json(), TilingScheme::cdb1_global_grid());
    let store = CdbDatastore::open(&root).unwrap();
    let restricted = RestrictedProfile::new();
    let report = store.validate(&restricted).unwrap();

    assert_eq!(
        report.class_coverage(RequirementsClass::Versioning),
        ContentCoverage::Unchecked,
        "the sweep marks undeclared content unchecked"
    );
    assert!(!report.class_passed(RequirementsClass::Versioning));

    let mut out = Vec::new();
    render(
        &report,
        &TextOptions {
            encoding: MetadataEncoding::Json,
            color: false,
            quiet: false,
            deny_warnings: false,
        },
        None,
        &mut out,
    )
    .unwrap();
    let text = String::from_utf8(out).unwrap();

    let row = row_for(&text, "versioning");
    assert!(row.contains("[FAIL]"), "{row:?}");
    assert!(
        row.contains("content present, no datastore-level check"),
        "a FAIL row keeps its coverage note: {row:?}"
    );
}

/// The line of `text` naming requirements class `class`.
fn row_for<'a>(text: &'a str, class: &str) -> &'a str {
    text.lines()
        .find(|line| line.trim_start().starts_with('[') && line.ends_with_class(class))
        .unwrap_or_else(|| panic!("no row for {class} in:\n{text}"))
}

/// Whether a rendered row names `class` as its class column.
trait RowExt {
    fn ends_with_class(&self, class: &str) -> bool;
}

impl RowExt for &str {
    fn ends_with_class(&self, class: &str) -> bool {
        match self.split_once(']') {
            Some((_, rest)) => rest.trim_start().starts_with(class),
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Colour
// ---------------------------------------------------------------------------

/// `--color` decides, `NO_COLOR` overrules, and `auto` asks the terminal.
/// Every combination is pinned because a report piped into a file with escape
/// sequences in it is a report nobody can diff.
#[test]
fn cli_check_colour_follows_the_flag_the_environment_and_the_terminal() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = bare(&profile);
    let root = root.to_string_lossy().into_owned();
    let base = ["--profile", "simulation", "--encoding", "json"];

    let coloured = |extra: &[&str], env: &Env| {
        let mut tokens = base.to_vec();
        tokens.extend_from_slice(extra);
        tokens.push(&root);
        lint(&tokens, env).out.contains('\u{1b}')
    };

    let tty = Env {
        no_color: false,
        stdout_is_terminal: true,
    };
    let no_color = Env {
        no_color: true,
        stdout_is_terminal: true,
    };

    assert!(coloured(&["--color", "always"], &plain_env()));
    assert!(!coloured(&["--color", "never"], &tty));
    assert!(coloured(&[], &tty), "auto follows the terminal");
    assert!(!coloured(&[], &plain_env()), "auto follows the terminal");
    assert!(!coloured(&[], &no_color), "NO_COLOR forces never");
    assert!(
        !coloured(&["--color", "always"], &no_color),
        "NO_COLOR forces never whatever the flag says (design §4)"
    );
}

/// Each status token carries its own colour, and the row's alignment is
/// unaffected by the invisible bytes.
#[test]
fn cli_check_each_status_token_has_its_own_colour() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = bare(&profile);
    fs::create_dir(root.join("My Tiles")).unwrap();
    fs::write(root.join("My Tiles").join("Payload.gpkg"), b"x").unwrap();

    let run = lint_sim(&root, &["--color", "always"]);

    for painted in [
        format!("{GREEN}[PASS]{RESET}"),
        format!("{RED}[FAIL]{RESET}"),
        format!("{DIM}[N/A]{RESET}"),
    ] {
        assert!(run.out.contains(&painted), "{painted:?}\n{}", run.out);
    }
}

// ---------------------------------------------------------------------------
// Detection: a diagnostic, never a yardstick (design §5 rule 5)
// ---------------------------------------------------------------------------

/// `--profile` without `--encoding` still fails — the yardstick is stated,
/// never guessed — but the error names the encoding the datastore appears to
/// use, so the second attempt is informed rather than a coin toss.
#[test]
fn req_cdb_lint_missing_encoding_suggests_without_choosing() {
    let (_tmp, root) = bare(&SimulationProfile::json());

    let run = lint(
        &["--profile", "simulation", &root.to_string_lossy()],
        &plain_env(),
    );

    assert_eq!(run.code, exit::USAGE, "{}", run.err);
    assert!(run.out.is_empty(), "stdout stays clean: {}", run.out);
    assert!(
        run.err.contains("global_metadata.json"),
        "the hint names the record it found: {}",
        run.err
    );
    assert!(
        run.err.contains("--encoding json"),
        "the hint names the flag to type: {}",
        run.err
    );
}

/// The suggestion is drawn from a fact, and where there is no fact there is
/// no suggestion: a root with neither record, or with both, gets the base
/// message. Guessing there would be inventing a yardstick.
#[test]
fn req_cdb_lint_no_encoding_hint_without_an_unambiguous_record() {
    let empty = tempfile::tempdir().unwrap();

    let (_tmp, ambiguous) = bare(&SimulationProfile::json());
    fs::write(
        ambiguous
            .join("global_metadata")
            .join("global_metadata.xml"),
        "<GlobalMetadata/>",
    )
    .unwrap();

    for root in [empty.path().to_path_buf(), ambiguous] {
        let run = lint(
            &["--profile", "simulation", &root.to_string_lossy()],
            &plain_env(),
        );

        assert_eq!(run.code, exit::USAGE, "{}", run.err);
        assert!(run.err.contains("`--encoding`"), "{}", run.err);
        assert!(
            !run.err.contains("you probably want"),
            "no fact, no hint: {}",
            run.err
        );
    }
}

/// A run that names the wrong encoding gets a hint **and keeps its verdict**.
/// The datastore still fails Requirement Metadata5, the exit code is still 1,
/// and the note lives on stderr where it cannot be mistaken for part of the
/// report. Deriving the encoding from the datastore instead would make
/// Metadata5 unfailable — the crate's own false-green failure mode,
/// reintroduced one layer up.
#[test]
fn req_cdb_lint_encoding_mismatch_hints_without_changing_the_verdict() {
    let (_tmp, root) = bare(&SimulationProfile::json());

    let run = lint(
        &[
            "--profile",
            "simulation",
            "--encoding",
            "xml",
            &root.to_string_lossy(),
        ],
        &plain_env(),
    );

    assert_eq!(run.code, exit::FINDINGS, "{}\n{}", run.out, run.err);
    assert!(run.out.contains("NON-CONFORMANT"), "{}", run.out);
    assert!(
        run.out.contains("/req/core/metadata-encoding"),
        "the finding stays in the report:\n{}",
        run.out
    );
    assert!(
        run.err.contains("--encoding json"),
        "the hint names the other spelling: {}",
        run.err
    );
    assert!(
        run.err.contains("unchanged"),
        "the hint says the verdict stands: {}",
        run.err
    );
}

/// The mismatch note speaks only when the global metadata record itself is
/// the other encoding. A stray wrongly-encoded file elsewhere carries the
/// same finding code, but the record on disk agrees with the yardstick — the
/// note's claim about "the datastore's global metadata record" would be
/// false, and the flag flip it suggests would convict the very record it
/// cites. The finding already names the stray file; the only honest hint
/// there is none at all.
#[test]
fn req_cdb_lint_encoding_hint_speaks_only_for_the_global_record() {
    let (_tmp, root) = bare(&SimulationProfile::json());
    let stray = root.join("Tiles").join("metadata");
    fs::create_dir_all(&stray).unwrap();
    fs::write(stray.join("Stray.xml"), "<resource_metadata/>").unwrap();

    let run = lint_sim(&root, &[]);

    assert_eq!(run.code, exit::FINDINGS, "{}\n{}", run.out, run.err);
    assert!(
        run.out.contains("/req/core/metadata-encoding"),
        "the stray file is still convicted:\n{}",
        run.out
    );
    assert!(
        !run.err.contains("re-run with"),
        "no flag flip is suggested while the record matches the declaration: {}",
        run.err
    );
}

// ---------------------------------------------------------------------------
// Operational failure (exit 3) and the stubs
// ---------------------------------------------------------------------------

/// A root that is not there is exit 3, not exit 1: "the tool could not look"
/// and "the datastore does not conform" are different facts, and CI needs to
/// tell them apart (design §4.1).
#[test]
fn cli_check_a_nonexistent_root_is_operational_not_a_finding() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("no-such-datastore");

    let run = lint(
        &[
            "--profile",
            "simulation",
            "--encoding",
            "json",
            &missing.to_string_lossy(),
        ],
        &plain_env(),
    );

    assert_eq!(run.code, exit::OPERATIONAL, "{}\n{}", run.out, run.err);
    assert!(run.out.is_empty(), "no half a report: {}", run.out);
    assert!(!run.err.is_empty(), "the failure is explained");
}

// `--profile-file` is no longer here: `tests/cli_profile.rs` owns the
// descriptor now that it resolves to a working yardstick, along with the
// pre-flight that refuses a broken one.

// `--baseline` is no longer here either: `tests/cli_baseline.rs` owns the
// ratchet now that it does something, including the honesty rule that keeps it
// from exiting 0 quietly. `--format json` and `-o` belong to
// `tests/cli_output.rs`, and `--format sarif` to `tests/cli_sarif.rs`.

// ---------------------------------------------------------------------------
// A profile that declares less than its datastore holds
// ---------------------------------------------------------------------------

/// [`SimulationProfile`]'s restrictions with Annex A's floor for a
/// declaration: the five mandatory classes and nothing else.
///
/// It exists for one test — [`req_cdb_lint_a_failing_row_still_prints_its_coverage_note`]
/// — because both shipped profiles declare every class, so neither can
/// produce the `FAIL`-with-`unchecked`-coverage row the content sweep files.
/// `tests/full_conformance.rs` in the library keeps the same fixture for the
/// same reason.
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

    fn tiling_scheme(&self) -> Option<TilingSchemeId> {
        self.base.tiling_scheme()
    }

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
