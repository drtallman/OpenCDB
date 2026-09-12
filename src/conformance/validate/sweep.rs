//! Stage 8 of [`super::validate`]: the content sweep and the two
//! declaration cross-checks.
//!
//! Stage 7 asks "does every class the profile declared hold up?"; this stage
//! asks the converse, "is there content whose class the profile never
//! declared?", reading only the [`ContentSignals`] the earlier passes already
//! collected — it adds no traversal of its own and never inspects payload
//! bytes.

use super::ContentSignals;
use crate::attribution::{self, AttributeModel};
use crate::conformance::{CdbViolation, ConformanceReport, RequirementsClass};
use crate::profiles::ApplicationProfile;
use crate::versioning;

/// Stage 8 — the content sweep (design spec §4): reports content whose
/// requirements class the profile did **not** declare, and runs the two
/// cross-checks that compare a profile's declaration against what the
/// datastore actually holds.
///
/// Declaration drives which classes get a *stage* (Annex A judges a datastore
/// against a profile's declaration), so this is the other half of that rule:
/// an undeclared class that turns out to govern real content is a
/// [`CdbViolation::DeclarationMismatch`] filed **under that class**, which
/// lists it in the report. A report therefore shows every declared class plus
/// any class content betrayed — see [`ConformanceReport`]'s own docs.
///
/// The Attr1-C name check runs regardless of declaration, because it judges a
/// file that exists rather than a class that was claimed: this is the sweep's
/// call to [`attribution::parse_file_name`], and the reason a
/// `vector_attributes.xsd` in an XML datastore or a mis-cased
/// `vector_attributes.JSON` in a JSON one is visible at all — Requirement
/// Metadata5's encoding sweep passes both (it reads `xsd` as XML and
/// lowercases extensions), and the facade reads only the canonical name.
pub(super) fn sweep_content(
    profile: &dyn ApplicationProfile,
    declared: &[RequirementsClass],
    signals: &ContentSignals,
    attribute_model: Option<&AttributeModel>,
    report: &mut ConformanceReport,
) {
    let undeclared = |class| !declared.contains(&class);

    // Attribution — a `vector_attributes.*` entry in `global_metadata/`.
    for name in &signals.attribute_model_files {
        // Attr1-C: the name itself, checked whatever the profile declares.
        // Per file, deliberately: this judges each entry that exists.
        if let Err(violation) = attribution::parse_file_name(name) {
            report.mark_content(RequirementsClass::Attribution);
            report.record_violation(violation.into());
        }
    }
    // The declaration question, by contrast, is asked once about the class —
    // two `vector_attributes.*` entries are still one undeclared class. The
    // `windingOrder` and `domainSet` signals take first-record-wins for the
    // same reason; this one reports the first entry as the evidence.
    if let Some(name) = signals.attribute_model_files.first()
        && undeclared(RequirementsClass::Attribution)
    {
        record_undeclared(
            profile,
            RequirementsClass::Attribution,
            "attribute model file",
            name.clone(),
            report,
        );
    }

    // Tiling — the `tilingScheme` element on the global record.
    if let Some(scheme) = signals.tiling_scheme.as_ref() {
        if undeclared(RequirementsClass::Tiling) {
            record_undeclared(
                profile,
                RequirementsClass::Tiling,
                "tilingScheme element",
                scheme.clone(),
                report,
            );
        }
        // Requirement Tiling4 (/req/core/tiling-tilingscheme-consistent,
        // §7.10.2.4): one tiling-scheme definition holds datastore-wide.
        // Within the datastore that is true by construction — there is one
        // element on the one global record — so the check that bites is
        // against the scheme the *profile* pins. A profile pinning no scheme
        // (the trait default) makes no claim to contradict; the element's own
        // conformance is then Tiling5/6/7's business.
        if let Some(declared_scheme) = profile.tiling_scheme()
            && declared_scheme.as_str() != scheme
        {
            report.record_violation(CdbViolation::DeclarationMismatch {
                profile: profile.name().to_owned(),
                class: RequirementsClass::Tiling,
                element: "tiling scheme",
                declared: declared_scheme.to_string(),
                found: scheme.clone(),
                clause: "/req/core/tiling-tilingscheme-consistent",
            });
        }
    }

    // Versioning — the `versions/` journal directory.
    if signals.versions_journal && undeclared(RequirementsClass::Versioning) {
        record_undeclared(
            profile,
            RequirementsClass::Versioning,
            "versions journal",
            format!("{}/", versioning::VERSIONS_DIR),
            report,
        );
    }

    // Topology — the Face4 `windingOrder` element on a resource record.
    if let Some(id) = signals.winding_order_record.as_ref()
        && undeclared(RequirementsClass::Topology)
    {
        record_undeclared(
            profile,
            RequirementsClass::Topology,
            "windingOrder element",
            format!("resource metadata record {id:?}"),
            report,
        );
    }

    // Coverages — the Coverages6 `domainSet` element on a resource record.
    if let Some(id) = signals.domain_set_record.as_ref()
        && undeclared(RequirementsClass::Coverages)
    {
        record_undeclared(
            profile,
            RequirementsClass::Coverages,
            "domainSet element",
            format!("resource metadata record {id:?}"),
            report,
        );
    }

    // Requirement Attr1-A (/req/core/attribute-model A, §7.1.2.1): a profile
    // "specifying and/or implementing attribution ... SHALL specify an
    // attribute model", and the datastore is required to be that model's
    // home. Compared only when both exist: a profile specifying no model
    // makes no claim, and a datastore holding none is the no-content case §4
    // makes pass. The on-disk side comes from the Attribution stage, the only
    // reader of the file, so an undeclared class has no model to compare —
    // it has already drawn the mismatch above.
    if let (Some(declared_model), Some(on_disk)) = (profile.attribute_model(), attribute_model)
        && &declared_model != on_disk
    {
        report.record_violation(CdbViolation::DeclarationMismatch {
            profile: profile.name().to_owned(),
            class: RequirementsClass::Attribution,
            element: "attribute model",
            declared: model_summary(&declared_model),
            found: model_summary(on_disk),
            clause: "/req/core/attribute-model",
        });
    }
}

/// Files an undeclared-content finding: a [`CdbViolation::DeclarationMismatch`]
/// under `class`, which both lists the class in the report and marks it
/// content-bearing — the content is real, it is the declaration that is
/// missing.
///
/// The cited clause is the class's own requirements-module URI: what the
/// content escapes by going undeclared is that module's requirements. (Annex
/// A's `/conf/minimal-core` is not it — that clause governs the *mandatory*
/// five, and its own violation is
/// [`CdbViolation::MissingConformanceDeclaration`].)
fn record_undeclared(
    profile: &dyn ApplicationProfile,
    class: RequirementsClass,
    element: &'static str,
    found: String,
    report: &mut ConformanceReport,
) {
    report.mark_content(class);
    report.record_violation(CdbViolation::DeclarationMismatch {
        profile: profile.name().to_owned(),
        class,
        element,
        declared: format!("no {} conformance class", class.as_str()),
        found,
        clause: class.requirements_uri(),
    });
}

/// A compact rendering of an attribute model for a finding message: the
/// supplementary Permission PAttr1 schema URI when present, then each
/// attribute as `id=name`. Descriptions are omitted to keep the message
/// readable, but the *comparison* that produced the message is on the whole
/// model — so two models differing only in a description mismatch while
/// rendering alike, and the reader is told which model is on disk rather than
/// which field differs.
fn model_summary(model: &AttributeModel) -> String {
    let attributes: Vec<String> = model
        .attributes
        .iter()
        .map(|attribute| format!("{}={}", attribute.id, attribute.name))
        .collect();
    match model.schema_uri.as_ref() {
        Some(uri) => format!("schemaUri={uri}, [{}]", attributes.join(", ")),
        None => format!("[{}]", attributes.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::support::{DeclaringProfile, fresh_store, model_of};
    use crate::attribution::AttributionViolation;
    use crate::coverage::DomainSet;
    use crate::datastore::{CdbDatastore, DatastoreSeed};
    use crate::metadata::ResourceMetadata;
    use crate::profiles::SimulationProfile;
    use crate::tiling::{TilingScheme, TilingSchemeId};
    use crate::topology::WindingOrder;
    use crate::versioning::PendingCollection;
    use std::fs;
    use tempfile::tempdir;

    /// Design spec §4 (the content sweep) — content whose requirements class
    /// the profile never declared is a
    /// [`CdbViolation::DeclarationMismatch`] **recorded under that class**,
    /// so the report lists it even though the profile did not. All five
    /// signals of §4's table fire at once here: a `vector_attributes.*` entry
    /// in `global_metadata/`, a `tilingScheme` on the global record, a
    /// `versions/` journal under the root, and the `windingOrder` and
    /// `domainSet` conditional elements on a resource metadata record.
    ///
    /// Geometry is deliberately absent from the table and so from this
    /// assertion: a record's Geom4 `uom` declares a *unit*, not the presence
    /// of geometry with m coordinates, which lives in a payload the crate
    /// does not decode — an inference too weak to convict a profile of an
    /// undeclared class.
    #[test]
    fn conf_core_sweep_reports_undeclared_content() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);

        // Attribution: the attribute model file.
        store
            .write_attribute_model(&model_of("1", "StreetName"))
            .unwrap();
        // Tiling: the tilingScheme element.
        let mut global = store.global_metadata().unwrap();
        global.tiling_scheme = Some(TilingScheme::cdb1_global_grid());
        global.write_to(store.layout()).unwrap();
        // Coverages + Topology: the two conditional elements on one record.
        let mut record = ResourceMetadata::new("Roads", "Road Network", "Tiled roads");
        record.keywords = vec!["transportation".to_owned()];
        record.domain_set = Some(DomainSet::new("m"));
        record.winding_order = Some(WindingOrder::Counterclockwise);
        store
            .write_resource_metadata("/Tiles/metadata/Roads.json", &record)
            .unwrap();
        // Versioning: the journal.
        store
            .apply_collection(PendingCollection::new().create("/Tiles/A.gpkg", *b"a1"))
            .unwrap();

        // Declared in full, every class passes and the sweep is silent.
        let report = store.validate(&DeclaringProfile::all()).unwrap();
        assert!(report.is_conformant(), "{report}");

        // Declared mandatory-only, each signal convicts its own class.
        let report = store.validate(&DeclaringProfile::mandatory_only()).unwrap();
        assert!(!report.is_conformant(), "{report}");
        for class in [
            RequirementsClass::Attribution,
            RequirementsClass::Coverages,
            RequirementsClass::Tiling,
            RequirementsClass::Topology,
            RequirementsClass::Versioning,
        ] {
            assert!(!report.class_passed(class), "{class}: {report}");
            assert!(report.class_has_content(class), "{class}: {report}");
            assert!(
                report.violations(class).iter().any(|violation| matches!(
                    violation,
                    CdbViolation::DeclarationMismatch { class: found, .. } if *found == class
                )),
                "{class}: {report}"
            );
        }
        // An undeclared class is listed only because content betrayed it.
        let listed: Vec<RequirementsClass> = report.classes().map(|(class, _)| class).collect();
        assert!(listed.contains(&RequirementsClass::Tiling), "{report}");
        assert!(!listed.contains(&RequirementsClass::Geometry), "{report}");
        // The mandatory five are untouched by the sweep.
        for &class in RequirementsClass::MANDATORY {
            assert!(report.class_passed(class), "{class}: {report}");
        }
    }

    /// Requirement Attr1-C (/req/core/attribute-model C, §7.1.2.1) — the
    /// sweep runs every `vector_attributes.*` entry of `global_metadata/`
    /// through [`crate::attribution::parse_file_name`], which is what makes a
    /// mis-named attribute model visible at all. Both escapes it closes are
    /// exercised: `vector_attributes.xsd` in an XML datastore (Requirement
    /// Metadata5 reads `xsd` as the XML encoding, so the encoding sweep
    /// passes it) and a mis-cased `vector_attributes.JSON` in a JSON one (the
    /// encoding sweep lowercases extensions, so it passes that too).
    #[test]
    fn req_core_attribute_model_file_name_swept() {
        // `vector_attributes.xsd` on an XML datastore.
        let tmp = tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::xml(),
            DatastoreSeed::new("id", "Title", "Description", "contact"),
        )
        .unwrap();
        fs::write(
            store
                .layout()
                .global_metadata_dir()
                .join("vector_attributes.xsd"),
            "<attributeModel/>",
        )
        .unwrap();
        let profile = DeclaringProfile {
            inner: SimulationProfile::xml(),
            ..DeclaringProfile::all()
        };
        let report = store.validate(&profile).unwrap();
        assert!(
            report
                .violations(RequirementsClass::Attribution)
                .iter()
                .any(|violation| matches!(
                    violation,
                    CdbViolation::Attribution(AttributionViolation::InvalidFileName { .. })
                )),
            "{report}"
        );

        // A mis-cased `vector_attributes.JSON` on a JSON datastore. Attr1-C
        // mandates one literal name, so the extension's case is not a matter
        // of taste: on a case-sensitive volume the facade could not read it.
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);
        let canonical = store
            .write_attribute_model(&model_of("1", "StreetName"))
            .unwrap();
        fs::rename(
            &canonical,
            store
                .layout()
                .global_metadata_dir()
                .join("vector_attributes.JSON"),
        )
        .unwrap();
        let report = store.validate(&DeclaringProfile::all()).unwrap();
        assert!(
            report
                .violations(RequirementsClass::Attribution)
                .iter()
                .any(|violation| matches!(
                    violation,
                    CdbViolation::Attribution(AttributionViolation::InvalidFileName { .. })
                )),
            "{report}"
        );
    }

    /// Design spec §4 — a class is convicted of undeclared content **once**,
    /// however many files carry the signal. The `windingOrder` and
    /// `domainSet` signals take first-record-wins for exactly this reason;
    /// the attribute-model signal is a list of directory entries, so two
    /// `vector_attributes.*` files in `global_metadata/` must still produce
    /// one `DeclarationMismatch` for Attribution, not one per file. The
    /// per-file Attr1-C name check is the opposite: it judges each file, so
    /// both mis-named files are convicted separately.
    #[test]
    fn conf_core_sweep_convicts_an_undeclared_class_once_per_class() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);
        store
            .write_attribute_model(&model_of("1", "StreetName"))
            .unwrap();
        // A second entry claiming the same stem. Attr1-C rejects its name
        // (`xsd` is not one of the two extensions the requirement admits),
        // but the *declaration* question is asked once about the class.
        fs::write(
            store
                .layout()
                .global_metadata_dir()
                .join("vector_attributes.xsd"),
            "<attributeModel/>",
        )
        .unwrap();

        let report = store.validate(&DeclaringProfile::mandatory_only()).unwrap();
        let mismatches = report
            .violations(RequirementsClass::Attribution)
            .iter()
            .filter(|violation| matches!(violation, CdbViolation::DeclarationMismatch { .. }))
            .count();
        assert_eq!(mismatches, 1, "{report}");
        // The per-file check still fires for the mis-named entry.
        assert_eq!(
            report
                .violations(RequirementsClass::Attribution)
                .iter()
                .filter(|violation| matches!(
                    violation,
                    CdbViolation::Attribution(AttributionViolation::InvalidFileName { .. })
                ))
                .count(),
            1,
            "{report}"
        );
    }

    /// Requirement Tiling4 (/req/core/tiling-tilingscheme-consistent,
    /// §7.10.2.4) — the profile's declared tiling scheme is cross-checked
    /// against the `tilingScheme` element on the global record; a datastore
    /// tiled by a scheme other than the one its profile pins is a
    /// `DeclarationMismatch` under Tiling, the same shape the CRS3 and
    /// Metadata2/5/8 cross-checks use.
    #[test]
    fn req_core_tiling_scheme_declaration_mismatch_detected() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);
        let profile = DeclaringProfile::all().with_scheme(TilingSchemeId::Cdb1GlobalGrid);

        let mut global = store.global_metadata().unwrap();
        global.tiling_scheme = Some(TilingScheme::gnosis_global_grid());
        global.write_to(store.layout()).unwrap();

        let report = store.validate(&profile).unwrap();
        assert!(!report.class_passed(RequirementsClass::Tiling), "{report}");
        assert!(
            report
                .violations(RequirementsClass::Tiling)
                .iter()
                .any(|violation| matches!(
                    violation,
                    CdbViolation::DeclarationMismatch {
                        element: "tiling scheme",
                        ..
                    }
                )),
            "{report}"
        );

        // The scheme the profile pins draws no finding.
        let mut global = store.global_metadata().unwrap();
        global.tiling_scheme = Some(TilingScheme::cdb1_global_grid());
        global.write_to(store.layout()).unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
    }

    /// Requirement Attr1-A (/req/core/attribute-model A, §7.1.2.1) — the
    /// attribute model a profile specifies is cross-checked against the model
    /// actually on disk; a datastore carrying a different model is a
    /// `DeclarationMismatch` under Attribution. A profile that specifies no
    /// model (the trait default) makes no claim and draws no finding.
    #[test]
    fn req_core_attribute_model_declaration_mismatch_detected() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);
        let profile = DeclaringProfile::all().with_model(model_of("1", "StreetName"));

        store
            .write_attribute_model(&model_of("2", "StreetWidth"))
            .unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(
            !report.class_passed(RequirementsClass::Attribution),
            "{report}"
        );
        assert!(
            report
                .violations(RequirementsClass::Attribution)
                .iter()
                .any(|violation| matches!(
                    violation,
                    CdbViolation::DeclarationMismatch {
                        element: "attribute model",
                        ..
                    }
                )),
            "{report}"
        );

        // The model the profile specifies draws no finding.
        store
            .write_attribute_model(&model_of("1", "StreetName"))
            .unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
    }
}
