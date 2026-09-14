//! Turning a [`ProfileChoice`] into the yardstick a run is measured against.
//!
//! The standard makes the application profile the implementable unit (§5.1),
//! so this module answers exactly one question — *which* profile — and never
//! the question honesty rule 5 forbids: it does not look at the datastore.
//! Both halves of a built-in yardstick come from the command line, the whole
//! of a described one comes from a document, and a datastore that disagrees
//! with either draws a finding rather than changing the yardstick.
//!
//! # The descriptor
//!
//! A tool offering only the two profiles compiled into it serves only the
//! people who already use this crate. [`DescriptorProfile`] is the third
//! implementation of [`ApplicationProfile`], read at runtime from the JSON
//! document design §9.1 specifies, so an implementer states their own
//! profile's restrictions in a file and is judged against them.
//!
//! Two properties of that document are load-bearing:
//!
//! - **Its vocabulary is the crate's.** Every closed-set field is read by the
//!   library's own parser — [`MetadataStandard::parse`],
//!   [`MetadataEncoding::parse`], [`UnitOfMeasure::parse`],
//!   [`TilingSchemeId::parse`], [`RequirementsClass::as_str`] — or, where the
//!   library offers no parser, by matching its own [`std::fmt::Display`]
//!   spellings. A second vocabulary would be a second thing to keep in step.
//! - **Unknown keys are rejected.** A typo that silently fell back to a
//!   default would produce the wrong yardstick, which is worse than a failed
//!   run: the yardstick is the one thing a conformance tool must not get
//!   quietly wrong.
//!
//! # Pre-flight
//!
//! Two [`ApplicationProfile`] methods return a `Result`, and a declared
//! attribute model can be invalid. [`DescriptorProfile::load`] exercises all
//! three before the datastore is opened, and a failure is a **usage** error
//! naming the descriptor field at fault. This is design §5 rule 6: letting
//! `validate` file the profile's own defects as CRS or Metadata violations
//! would convict the innocent party.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use rusty_cdb::attribution::{self, AttributeModel};
use rusty_cdb::conformance::RequirementsClass;
use rusty_cdb::crs::{CrsViolation, StorageCrs};
use rusty_cdb::hierarchy;
use rusty_cdb::metadata::{MetadataEncoding, MetadataStandard, UnitOfMeasure};
use rusty_cdb::naming::{self, CaseRule, StyleGuide};
use rusty_cdb::profiles::{ApplicationProfile, StorageTechnology, TilingSchemeId};
use rusty_cdb::{GnosisProfile, SimulationProfile};

use crate::cli::{BuiltinProfile, Encoding, ProfileChoice, UsageError};

/// Resolves the profile a run declared.
///
/// The return is boxed and `dyn` rather than an enum of the two built-ins,
/// because [`DescriptorProfile`] cannot be enumerated here: it is read at
/// runtime. Callers already pass `&*profile` to `CdbDatastore::validate`,
/// which takes `&dyn ApplicationProfile`, so the indirection costs nothing.
///
/// A yardstick this build cannot construct is a **usage** error, never an
/// operational one: nothing was judged, and the reason is the request rather
/// than the datastore (design §4.1). That covers a descriptor that is absent,
/// unreadable, malformed, or — by way of [`DescriptorProfile::load`]'s
/// pre-flight — self-contradictory.
pub fn resolve(choice: &ProfileChoice) -> Result<Box<dyn ApplicationProfile>, UsageError> {
    match choice {
        ProfileChoice::Builtin { profile, encoding } => Ok(builtin(*profile, *encoding)),
        ProfileChoice::File(path) => Ok(Box::new(DescriptorProfile::load(path)?)),
    }
}

/// The four built-in pairings. Each is a distinct constructor rather than one
/// constructor taking an encoding, because that is the shape the library
/// offers: `SimulationProfile` makes the standard's third encoding, `gpkg`,
/// unrepresentable by refusing to accept an encoding value at all, and
/// cdb-lint's own `--encoding` vocabulary is narrowed to match (design §9.2).
fn builtin(profile: BuiltinProfile, encoding: Encoding) -> Box<dyn ApplicationProfile> {
    match (profile, encoding) {
        (BuiltinProfile::Simulation, Encoding::Json) => Box::new(SimulationProfile::json()),
        (BuiltinProfile::Simulation, Encoding::Xml) => Box::new(SimulationProfile::xml()),
        (BuiltinProfile::Gnosis, Encoding::Json) => Box::new(GnosisProfile::json()),
        (BuiltinProfile::Gnosis, Encoding::Xml) => Box::new(GnosisProfile::xml()),
    }
}

// ---------------------------------------------------------------------------
// The rosters the descriptor's vocabularies are drawn from
// ---------------------------------------------------------------------------

/// Every metadata encoding the library defines, in its declaration order.
///
/// [`MetadataEncoding`] carries no `ALL` constant, so the roster is written
/// out here rather than borrowed. `cli_profile_the_encoding_roster_is_whole`
/// holds it complete with an exhaustive `match`, which is a compile-time
/// check and not merely a test: a variant added to the library breaks the
/// build here rather than quietly becoming unspellable.
const METADATA_ENCODINGS: [MetadataEncoding; 3] = [
    MetadataEncoding::Xml,
    MetadataEncoding::Json,
    MetadataEncoding::Gpkg,
];

/// Every unit of measure the library defines, in its declaration order.
/// [`UnitOfMeasure`] carries no `ALL` constant either;
/// `cli_profile_the_uom_roster_is_whole` holds this one complete the same way.
const UNITS_OF_MEASURE: [UnitOfMeasure; 4] = [
    UnitOfMeasure::Meters,
    UnitOfMeasure::Feet,
    UnitOfMeasure::Kilometers,
    UnitOfMeasure::Miles,
];

/// Every storage technology the library defines.
///
/// Unlike the two rosters above, this one **cannot** be held complete from
/// outside the library: [`StorageTechnology`] is `#[non_exhaustive]`, so a
/// `match` over it here must carry a wildcard arm and an added variant would
/// compile. If the library ever names a second technology, this line has to be
/// updated by hand, and nothing in cdb-lint will say so.
const STORAGE_TECHNOLOGIES: [StorageTechnology; 1] = [StorageTechnology::FileSystem];

// ---------------------------------------------------------------------------
// The document
// ---------------------------------------------------------------------------

/// The JSON profile descriptor exactly as written (design §9.1).
///
/// Every closed-vocabulary field is a `String` here and is read into the
/// library's own type in [`Descriptor::into_profile`]. Letting serde do it
/// through a derived `Deserialize` would produce serde's diagnostic — "unknown
/// variant" and a list of Rust variant names — where design §4 wants the
/// spellings the field actually accepts.
///
/// `deny_unknown_fields` is the whole reason this type is spelled out rather
/// than read as a map: a mistyped key must fail the run, not fall back to a
/// default that quietly changes the yardstick.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    /// The profile's name, as the report will record it.
    name: String,
    /// The Name6 case rule, in [`CaseRule`]'s own spelling.
    case_rule: String,
    /// The Name3/Metadata4 language tag (BCP 47).
    language: String,
    /// Names exempted from the case rule (Requirement Name5). The library
    /// reserves the spec-mandated ones itself; these are the profile's own.
    #[serde(default)]
    reserved_names: Vec<String>,
    /// The storage CRS as WKT-2 text (Requirement CRS3/CRS5). Exclusive with
    /// [`Self::storage_crs_wkt_path`], and exactly one of the two is required.
    #[serde(default)]
    storage_crs_wkt: Option<String>,
    /// The storage CRS as a path to a WKT-2 file, resolved against the
    /// **descriptor's** directory.
    #[serde(default)]
    storage_crs_wkt_path: Option<PathBuf>,
    /// The Metadata2 metadata standard.
    metadata_standard: String,
    /// The Metadata5 metadata encoding.
    metadata_encoding: String,
    /// The Metadata8 unit of measure.
    uom: String,
    /// The Annex B storage technology.
    #[serde(default = "default_storage_technology")]
    storage_technology: String,
    /// The requirements classes the profile declares: `"all"`, `"mandatory"`,
    /// or an array of class tokens. Read as a raw value so the three forms can
    /// be told apart with a diagnostic that names all three.
    #[serde(default = "default_conformance_classes")]
    conformance_classes: serde_json::Value,
    /// The Tiling8 tiling scheme, if the profile declares one.
    #[serde(default)]
    tiling_scheme: Option<String>,
    /// The Name5 convention directory holding resource-metadata records.
    #[serde(default = "default_resource_metadata_dir")]
    resource_metadata_dir: String,
    /// The RFile1 root folder name.
    #[serde(default = "default_root_folder_name")]
    root_folder_name: String,
    /// Extensions outside the Name7 table the profile vouches for (Name7-B).
    #[serde(default)]
    known_extensions: Vec<String>,
    /// The Attr1-A attribute model, if the profile declares one. Nests the
    /// library's own type, so the block reads exactly like the
    /// `vector_attributes.json` the datastore will hold.
    #[serde(default)]
    attribute_model: Option<AttributeModel>,
}

/// Annex B's only storage technology, in the library's own spelling.
fn default_storage_technology() -> String {
    StorageTechnology::FileSystem.as_str().to_owned()
}

/// Every class, which is what a profile restricting the whole core (§5.1)
/// declares. Under-declaring is not the safe default: the content sweep
/// reports content whose class the profile never declared, so a profile that
/// declared less than it supports would convict its own datastores.
fn default_conformance_classes() -> serde_json::Value {
    serde_json::Value::String(ALL_CLASSES.to_owned())
}

/// The `metadata/` convention of the two shipped profiles.
fn default_resource_metadata_dir() -> String {
    "metadata".to_owned()
}

/// The root Recommendation RFile1 names, in the library's own spelling.
fn default_root_folder_name() -> String {
    hierarchy::RECOMMENDED_ROOT_NAME.to_owned()
}

/// The `conformance_classes` keyword for every class the core defines.
const ALL_CLASSES: &str = "all";

/// The `conformance_classes` keyword for Annex A's mandatory five.
const MANDATORY_CLASSES: &str = "mandatory";

// ---------------------------------------------------------------------------
// The profile
// ---------------------------------------------------------------------------

/// An application profile read from a JSON descriptor (design §9).
///
/// Nothing here is inferred. Every restriction the standard makes a profile
/// declare is a field of the document, and a document that leaves a required
/// one out does not load — because a profile the tool completed on the user's
/// behalf is a yardstick the user did not state.
///
/// Construct one with [`Self::load`], which is also where the pre-flight of
/// design §5 rule 6 runs.
#[derive(Debug, Clone)]
pub struct DescriptorProfile {
    name: String,
    case_rule: CaseRule,
    language: String,
    reserved_names: Vec<String>,
    storage_crs_wkt: String,
    metadata_standard: MetadataStandard,
    metadata_encoding: MetadataEncoding,
    uom: UnitOfMeasure,
    storage_technology: StorageTechnology,
    conformance_classes: Vec<RequirementsClass>,
    tiling_scheme: Option<TilingSchemeId>,
    resource_metadata_dir: String,
    root_folder_name: String,
    known_extensions: Vec<String>,
    attribute_model: Option<AttributeModel>,
}

impl DescriptorProfile {
    /// Read the descriptor at `path`, and prove the yardstick it describes is
    /// usable before anything is judged against it.
    ///
    /// # Errors
    ///
    /// A [`UsageError`] — exit 2 — for every way this can fail: the document
    /// is absent, is not JSON, carries an unknown key, omits a required field,
    /// spells a closed-vocabulary value wrongly, names zero or two storage
    /// CRSs, or describes a profile whose own accessors fail (the pre-flight
    /// of design §5 rule 6, run here before the caller may open a datastore).
    /// None of these is a fact about a datastore, and none of them may reach a
    /// conformance report.
    pub fn load(path: &Path) -> Result<Self, UsageError> {
        let text = fs::read_to_string(path)
            .map_err(|error| invalid(path, format_args!("cannot be read: {error}")))?;
        let descriptor: Descriptor = serde_json::from_str(&text)
            .map_err(|error| invalid(path, format_args!("is not a valid descriptor: {error}")))?;

        let mut profile = descriptor.into_profile(path)?;
        profile.preflight(path)?;

        Ok(profile)
    }

    /// Exercise every fallible part of the yardstick, before the datastore is
    /// opened (design §5 rule 6).
    ///
    /// [`ApplicationProfile::storage_crs`] and [`ApplicationProfile::language`]
    /// both return a `Result`, and `validate` calls both. A descriptor whose
    /// WKT does not parse or whose language tag is malformed would therefore
    /// be reported as a CRS or a Metadata violation *against the datastore* —
    /// convicting the innocent party. Calling them here turns each into a
    /// usage error naming the descriptor field at fault.
    ///
    /// A declared attribute model gets the same treatment through the
    /// library's own [`attribution::validate_attribute_model_document`], the
    /// datastore-level entry point for exactly this document, so a descriptor's
    /// model is judged by precisely the rule an on-disk one is judged by. The
    /// canonical model it returns replaces the parsed one: the conformance
    /// cross-check of Requirement Attr1-A compares the profile's declared
    /// model against the datastore's, and the datastore's has been through the
    /// same canonicalization, so a declared model carrying one stray space
    /// would otherwise convict a datastore the crate itself had written from
    /// it.
    fn preflight(&mut self, path: &Path) -> Result<(), UsageError> {
        if self.metadata_encoding == MetadataEncoding::Gpkg {
            return Err(invalid(
                path,
                format_args!(
                    "declares `metadata_encoding` `gpkg`. The token belongs to Requirement \
                 Metadata5's vocabulary, but this build implements no GeoPackage metadata \
                 container: writing one answers `UnsupportedEncoding`. The refusal is \
                 deliberate, not a gap — the fault is in the request rather than in any \
                 datastore, so it exits 2 and not the code that means the datastore could not \
                 be read. Lifting the stance is future work (TDD_PLAN §8). Use `json` or `xml`"
                ),
            ));
        }

        if let Err(violation) = self.storage_crs() {
            return Err(invalid(
                path,
                format_args!(
                    "declares a storage CRS that is not one: {violation}. The WKT came from \
                 `storage_crs_wkt`/`storage_crs_wkt_path`, so this is a defect of the yardstick \
                 and not of any datastore"
                ),
            ));
        }

        if let Err(violation) = self.language() {
            return Err(invalid(
                path,
                format_args!("declares a `language` that is not a language tag: {violation}"),
            ));
        }

        if let Some(model) = self.attribute_model.take() {
            self.attribute_model = Some(self.checked_attribute_model(path, model)?);
        }

        Ok(())
    }

    /// Put a declared attribute model through the library's document check and
    /// answer with the canonical form.
    ///
    /// The model is re-serialized as JSON and handed to
    /// [`attribution::validate_attribute_model_document`] under the Attr1-C
    /// file name, rather than validated in place: that function is the path an
    /// on-disk model takes, it canonicalizes before it validates, and using it
    /// is what makes a descriptor's model and a datastore's model the same
    /// kind of thing. The JSON encoding is used whatever the profile declares,
    /// because Attr1-C's content rules are encoding-independent and the
    /// descriptor is itself a JSON document.
    fn checked_attribute_model(
        &self,
        path: &Path,
        model: AttributeModel,
    ) -> Result<AttributeModel, UsageError> {
        let document = model.to_json_string().map_err(|error| {
            invalid(
                path,
                format_args!("declares an `attribute_model` that cannot be encoded: {error}"),
            )
        })?;
        let file_name = attribution::file_name_for(MetadataEncoding::Json)
            .unwrap_or_else(|| format!("{}.json", attribution::VECTOR_ATTRIBUTES_STEM));

        attribution::validate_attribute_model_document(&file_name, &document).map_err(|violation| {
            invalid(
                path,
                format_args!("declares an invalid `attribute_model`: {violation}"),
            )
        })
    }
}

impl Descriptor {
    /// Read the document's text into the library's own types.
    ///
    /// `path` is the descriptor's own path: it names the document in every
    /// diagnostic, and its directory is what a relative
    /// `storage_crs_wkt_path` resolves against.
    fn into_profile(self, path: &Path) -> Result<DescriptorProfile, UsageError> {
        require_report_name(&self.name).map_err(|error| error.at(path))?;
        require_component("resource_metadata_dir", &self.resource_metadata_dir)
            .map_err(|error| error.at(path))?;
        require_component("root_folder_name", &self.root_folder_name)
            .map_err(|error| error.at(path))?;
        for entry in &self.reserved_names {
            require_entry_component("reserved_names", entry).map_err(|error| error.at(path))?;
        }
        for entry in &self.known_extensions {
            require_extension_entry(entry).map_err(|error| error.at(path))?;
        }
        let case_rule = parse_case_rule(&self.case_rule).map_err(|error| error.at(path))?;
        let metadata_standard =
            parse_metadata_standard(&self.metadata_standard).map_err(|error| error.at(path))?;
        let metadata_encoding =
            parse_metadata_encoding(&self.metadata_encoding).map_err(|error| error.at(path))?;
        let uom = parse_uom(&self.uom).map_err(|error| error.at(path))?;
        let storage_technology =
            parse_storage_technology(&self.storage_technology).map_err(|error| error.at(path))?;
        let tiling_scheme = match &self.tiling_scheme {
            Some(scheme) => Some(parse_tiling_scheme(scheme).map_err(|error| error.at(path))?),
            None => None,
        };
        let conformance_classes =
            parse_conformance_classes(&self.conformance_classes).map_err(|error| error.at(path))?;
        let storage_crs_wkt =
            read_storage_crs(path, self.storage_crs_wkt, self.storage_crs_wkt_path)?;

        Ok(DescriptorProfile {
            name: self.name,
            case_rule,
            language: self.language,
            reserved_names: self.reserved_names,
            storage_crs_wkt,
            metadata_standard,
            metadata_encoding,
            uom,
            storage_technology,
            conformance_classes,
            tiling_scheme,
            resource_metadata_dir: self.resource_metadata_dir,
            root_folder_name: self.root_folder_name,
            known_extensions: self.known_extensions,
            attribute_model: self.attribute_model,
        })
    }
}

impl ApplicationProfile for DescriptorProfile {
    fn name(&self) -> &str {
        &self.name
    }

    /// The Name6 case rule and Name3 language, plus every name the descriptor
    /// exempts from the rule. The library reserves the names the spec mandates
    /// verbatim on its own; the declared convention directory is reserved
    /// here automatically — exactly as `SimulationProfile::style_guide`
    /// reserves its own `metadata` (a Requirement Name5 duty), because a
    /// yardstick that convicted the layout its own `resource_metadata_dir`
    /// declares would contradict itself. `reserved_names` carries only the
    /// profile's *extra* exemptions.
    fn style_guide(&self) -> StyleGuide {
        let mut guide = StyleGuide::new(self.case_rule, self.language.clone());
        guide.reserve_name(self.resource_metadata_dir.clone());
        for name in &self.reserved_names {
            guide.reserve_name(name.clone());
        }
        guide
    }

    fn storage_crs(&self) -> Result<StorageCrs, CrsViolation> {
        StorageCrs::from_wkt(&self.storage_crs_wkt)
    }

    fn metadata_standard(&self) -> MetadataStandard {
        self.metadata_standard
    }

    fn metadata_encoding(&self) -> MetadataEncoding {
        self.metadata_encoding
    }

    fn uom(&self) -> UnitOfMeasure {
        self.uom
    }

    fn storage_technology(&self) -> StorageTechnology {
        self.storage_technology
    }

    fn conformance_classes(&self) -> Vec<RequirementsClass> {
        self.conformance_classes.clone()
    }

    /// A path is a resource-metadata record when its parent component is the
    /// descriptor's convention directory and its extension is one of the
    /// Metadata5 encodings — the convention `SimulationProfile` implements,
    /// with the directory name supplied by the document.
    ///
    /// Both matches fold ASCII case. This is a path *guard*, and
    /// `docs/CONFORMANCE.md` §9 makes folding the crate-wide stance for
    /// guards while requirements stay exact: on a case-insensitive filesystem
    /// `Metadata/` is the same directory, and a byte-exact recognizer would
    /// leave every record inside it unread — no Metadata and no Links findings
    /// at all. Whole components only, so a directory that merely begins the
    /// same way is ordinary content.
    fn is_resource_metadata(&self, logical_path: &str) -> bool {
        let relative = logical_path.strip_prefix('/').unwrap_or(logical_path);
        let mut components = relative.rsplit('/');
        let file_name = components.next().unwrap_or_default();
        if !components
            .next()
            .is_some_and(|dir| dir.eq_ignore_ascii_case(&self.resource_metadata_dir))
        {
            return false;
        }
        match naming::split_extension(file_name).1 {
            Some(extension) => METADATA_ENCODINGS
                .iter()
                .any(|known| known.extension().eq_ignore_ascii_case(extension)),
            None => false,
        }
    }

    fn root_folder_name(&self) -> &str {
        &self.root_folder_name
    }

    fn tiling_scheme(&self) -> Option<TilingSchemeId> {
        self.tiling_scheme
    }

    fn attribute_model(&self) -> Option<AttributeModel> {
        self.attribute_model.clone()
    }

    fn known_extensions(&self) -> Vec<String> {
        self.known_extensions.clone()
    }
}

// ---------------------------------------------------------------------------
// Reading the document's values
// ---------------------------------------------------------------------------

/// A rejected field value, before it knows which document it came from.
///
/// Splitting the message this way keeps every vocabulary function pure and
/// testable on its own, while still letting the finished diagnostic name the
/// descriptor: [`Self::at`] supplies the path once, at the one place that has
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FieldError {
    message: String,
}

impl FieldError {
    /// Attach the descriptor the field came from.
    fn at(self, path: &Path) -> UsageError {
        invalid(path, format_args!("{}", self.message))
    }
}

/// The profile's `name`, which the report prints verbatim in its header,
/// records as its `profile` field, and compares as a baseline's identity.
///
/// A blank name says nothing in all three places. A control character is
/// worse than nothing: a newline forges a second header line a reader has
/// no way to distrust, in a report whose whole job is to be quotable.
fn require_report_name(value: &str) -> Result<(), FieldError> {
    if value.trim().is_empty() {
        return Err(FieldError {
            message: "gives `name` as a blank string; the name is the report's `profile` \
                      field and a baseline's identity, so it has to say something"
                .to_owned(),
        });
    }
    if value.chars().any(char::is_control) {
        return Err(FieldError {
            message: format!(
                "gives `name` as {value:?}, which carries a control character; the name is \
                 printed verbatim in every report header, where a newline would forge a \
                 second line"
            ),
        });
    }
    Ok(())
}

/// A field whose value is matched against one path component: non-empty and
/// free of the `/` separator, because a component is never empty and never
/// contains one.
///
/// A value that cannot match is not a stricter yardstick — it is a dead
/// one. `resource_metadata_dir: "metadata/"` recognizes no record at all,
/// so the very violations the field exists to route go unread and the
/// report claims more conformance than was checked: the false green of
/// honesty rule 1, reached through one character in a descriptor.
fn require_component(field: &str, value: &str) -> Result<(), FieldError> {
    if value.is_empty() {
        return Err(FieldError {
            message: format!(
                "gives `{field}` as an empty string, which no directory component can \
                 ever equal, so the field would silently match nothing"
            ),
        });
    }
    if value.contains('/') {
        return Err(FieldError {
            message: format!(
                "gives `{field}` as `{value}`, which contains a path separator and can \
                 never equal one directory component, so the field would silently match \
                 nothing"
            ),
        });
    }
    Ok(())
}

/// One entry of a component-matched list field, held to the same rule as
/// [`require_component`] with the entry named in the diagnostic.
fn require_entry_component(field: &str, entry: &str) -> Result<(), FieldError> {
    require_component(field, entry).map_err(|_| FieldError {
        message: format!(
            "gives `{field}` an entry `{entry}` that no path component can ever equal — \
             a component is never empty and never contains `/` — so the entry would \
             silently exempt nothing"
        ),
    })
}

/// One entry of `known_extensions`: matched against a file's extension,
/// which never contains a dot or a separator and is never empty. An entry
/// that cannot match vouches for nothing, and the author finds out under
/// `--deny-warnings`, when the build fails on warnings they meant to
/// silence — so the inert spelling is refused here, where the fix is named.
fn require_extension_entry(entry: &str) -> Result<(), FieldError> {
    if entry.is_empty() || entry.contains('/') {
        return Err(FieldError {
            message: format!(
                "gives `known_extensions` an entry `{entry}` that no file extension can \
                 ever equal, so the entry would vouch for nothing"
            ),
        });
    }
    if entry.contains('.') {
        return Err(FieldError {
            message: format!(
                "gives `known_extensions` an entry `{entry}` — an extension is spelled \
                 without its dot (`{}`), and a dotted entry can never match one, so it \
                 would vouch for nothing",
                entry.trim_start_matches('.')
            ),
        });
    }
    Ok(())
}

/// The diagnostic for a value outside a closed vocabulary.
///
/// It always lists the spellings that would have worked. An error saying only
/// that a value is invalid makes the user go and look up what the tool already
/// knew, and the descriptor's vocabularies are small enough to print.
fn rejected<'spelling>(
    field: &str,
    value: &str,
    accepted: impl IntoIterator<Item = &'spelling str>,
) -> FieldError {
    FieldError {
        message: format!(
            "gives `{field}` as `{value}`, which is not one of {}",
            spellings(accepted)
        ),
    }
}

/// A vocabulary as a readable list: `` `a` ``, `` `a` or `b` ``,
/// `` `a`, `b`, or `c` ``.
fn spellings<'spelling>(accepted: impl IntoIterator<Item = &'spelling str>) -> String {
    let quoted: Vec<String> = accepted
        .into_iter()
        .map(|spelling| format!("`{spelling}`"))
        .collect();
    match quoted.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, [first])) => format!("{first} or {last}"),
        Some((last, rest)) => format!("{}, or {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// `case_rule`: [`CaseRule`] has no parser, so the accepted spellings are its
/// own [`fmt::Display`] output — the spec's spellings, §7.4.7.
fn parse_case_rule(value: &str) -> Result<CaseRule, FieldError> {
    CaseRule::ALL
        .into_iter()
        .find(|rule| rule.to_string() == value)
        .ok_or_else(|| {
            let accepted: Vec<String> = CaseRule::ALL.iter().map(CaseRule::to_string).collect();
            rejected("case_rule", value, accepted.iter().map(String::as_str))
        })
}

/// `metadata_standard`: Requirement Metadata2's vocabulary, the library's.
fn parse_metadata_standard(value: &str) -> Result<MetadataStandard, FieldError> {
    MetadataStandard::parse(value).map_err(|_| {
        rejected(
            "metadata_standard",
            value,
            MetadataStandard::ALL.iter().map(|s| s.as_str()),
        )
    })
}

/// `metadata_encoding`: Requirement Metadata5's vocabulary, the library's.
///
/// All three spellings are accepted here, `gpkg` included, because all three
/// are the requirement's. This build implements only two of them, and the
/// pre-flight of [`DescriptorProfile::preflight`] says so in its own words
/// rather than pretending the third is a typo.
fn parse_metadata_encoding(value: &str) -> Result<MetadataEncoding, FieldError> {
    MetadataEncoding::parse(value).map_err(|_| {
        rejected(
            "metadata_encoding",
            value,
            METADATA_ENCODINGS.iter().map(|e| e.as_str()),
        )
    })
}

/// `uom`: Requirement Metadata8's vocabulary, the library's.
fn parse_uom(value: &str) -> Result<UnitOfMeasure, FieldError> {
    UnitOfMeasure::parse(value)
        .map_err(|_| rejected("uom", value, UNITS_OF_MEASURE.iter().map(|u| u.as_str())))
}

/// `tiling_scheme`: Recommendation Tiling1's vocabulary, the library's.
fn parse_tiling_scheme(value: &str) -> Result<TilingSchemeId, FieldError> {
    TilingSchemeId::parse(value).map_err(|_| {
        rejected(
            "tiling_scheme",
            value,
            TilingSchemeId::ALL.iter().map(|s| s.as_str()),
        )
    })
}

/// `storage_technology`: Annex B's vocabulary. The library offers no parser
/// and the enum is `#[non_exhaustive]`, so the roster is [`STORAGE_TECHNOLOGIES`]
/// and the spellings are still the library's own.
fn parse_storage_technology(value: &str) -> Result<StorageTechnology, FieldError> {
    STORAGE_TECHNOLOGIES
        .into_iter()
        .find(|technology| technology.as_str() == value)
        .ok_or_else(|| {
            rejected(
                "storage_technology",
                value,
                STORAGE_TECHNOLOGIES.iter().map(|t| t.as_str()),
            )
        })
}

/// `conformance_classes`: `"all"`, `"mandatory"`, or an array of class tokens.
///
/// The declaration decides which validation stages run and which content the
/// sweep reports as undeclared, so a misspelled token would silently narrow
/// the audit. Every rejection therefore names the token and the vocabulary.
fn parse_conformance_classes(
    value: &serde_json::Value,
) -> Result<Vec<RequirementsClass>, FieldError> {
    match value {
        serde_json::Value::String(keyword) if keyword == ALL_CLASSES => {
            Ok(RequirementsClass::ALL.to_vec())
        }
        serde_json::Value::String(keyword) if keyword == MANDATORY_CLASSES => {
            Ok(RequirementsClass::MANDATORY.to_vec())
        }
        serde_json::Value::String(keyword) => Err(rejected(
            "conformance_classes",
            keyword,
            [ALL_CLASSES, MANDATORY_CLASSES],
        )),
        serde_json::Value::Array(tokens) => tokens.iter().map(parse_class_token).collect(),
        _ => Err(FieldError {
            message: format!(
                "gives `conformance_classes` as neither a keyword ({}) nor a list of class tokens",
                spellings([ALL_CLASSES, MANDATORY_CLASSES])
            ),
        }),
    }
}

/// One entry of a `conformance_classes` array.
fn parse_class_token(token: &serde_json::Value) -> Result<RequirementsClass, FieldError> {
    let Some(token) = token.as_str() else {
        return Err(FieldError {
            message: format!(
                "gives `conformance_classes` an entry that is not a class token ({token}); the \
                 tokens are {}",
                spellings(RequirementsClass::ALL.iter().map(|c| c.as_str()))
            ),
        });
    };
    RequirementsClass::ALL
        .iter()
        .copied()
        .find(|class| class.as_str() == token)
        .ok_or_else(|| {
            rejected(
                "conformance_classes",
                token,
                RequirementsClass::ALL.iter().map(|c| c.as_str()),
            )
        })
}

/// The storage CRS's WKT-2 text, from whichever of the two fields carries it.
///
/// Exactly one is required. Neither leaves the profile without the single CRS
/// Requirement CRS3 makes it declare; both leave two, with no rule for which
/// wins — and a tool that picked one would be choosing a yardstick.
///
/// A relative `storage_crs_wkt_path` resolves against the **descriptor's own
/// directory**, never the working directory. A descriptor is a document a team
/// checks in and shares, and resolving its references against whatever
/// directory the operator happened to be standing in would make it mean
/// different things in different shells.
fn read_storage_crs(
    path: &Path,
    inline: Option<String>,
    reference: Option<PathBuf>,
) -> Result<String, UsageError> {
    match (inline, reference) {
        (Some(wkt), None) => Ok(wkt),
        (None, Some(relative)) => {
            let resolved = path.parent().unwrap_or(Path::new("")).join(&relative);
            fs::read_to_string(&resolved).map_err(|error| {
                invalid(
                    path,
                    format_args!(
                        "gives `storage_crs_wkt_path` as `{}`, which resolves to {} beside the \
                         descriptor and cannot be read: {error}",
                        relative.display(),
                        resolved.display()
                    ),
                )
            })
        }
        (Some(_), Some(_)) => Err(invalid(
            path,
            format_args!(
                "gives both `storage_crs_wkt` and `storage_crs_wkt_path`; a profile declares one \
                 storage CRS (Requirement CRS3), so exactly one of the two is required"
            ),
        )),
        (None, None) => Err(invalid(
            path,
            format_args!(
                "gives neither `storage_crs_wkt` nor `storage_crs_wkt_path`; a profile declares \
                 one storage CRS (Requirement CRS3), so exactly one of the two is required"
            ),
        )),
    }
}

/// The house shape of every descriptor diagnostic: the document, then what is
/// wrong with it. Naming the file matters when a CI job runs several.
fn invalid(path: &Path, detail: fmt::Arguments<'_>) -> UsageError {
    UsageError::new(format!(
        "the profile descriptor {} {detail}",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use rusty_cdb::profiles::simulation::WGS84_2D_WKT;

    /// The resolved profile for a built-in pairing that must resolve.
    fn builtin_profile(profile: BuiltinProfile, encoding: Encoding) -> Box<dyn ApplicationProfile> {
        match resolve(&ProfileChoice::Builtin { profile, encoding }) {
            Ok(resolved) => resolved,
            Err(error) => panic!("{profile:?}/{encoding:?} should resolve: {error}"),
        }
    }

    /// Every one of the four pairings resolves, and each resolves to the
    /// profile it names rather than to a default that happens to be close.
    /// The encoding is checked through the profile's own declaration, which
    /// is what `validate` will compare the datastore against.
    #[test]
    fn cli_profile_every_builtin_pairing_resolves_to_what_it_names() {
        for (profile, encoding, name, declared) in [
            (
                BuiltinProfile::Simulation,
                Encoding::Json,
                "simulation",
                MetadataEncoding::Json,
            ),
            (
                BuiltinProfile::Simulation,
                Encoding::Xml,
                "simulation",
                MetadataEncoding::Xml,
            ),
            (
                BuiltinProfile::Gnosis,
                Encoding::Json,
                "gnosis",
                MetadataEncoding::Json,
            ),
            (
                BuiltinProfile::Gnosis,
                Encoding::Xml,
                "gnosis",
                MetadataEncoding::Xml,
            ),
        ] {
            let resolved = builtin_profile(profile, encoding);

            assert_eq!(resolved.name(), name, "{profile:?}/{encoding:?}");
            assert_eq!(
                resolved.metadata_encoding(),
                declared,
                "{profile:?}/{encoding:?}"
            );
        }
    }

    /// The two profiles differ in the tiling scheme they pin, which is the
    /// whole reason the second one exists. Resolving them to the same
    /// yardstick would make `--profile` decorative.
    #[test]
    fn cli_profile_the_two_builtins_are_different_yardsticks() {
        let simulation = builtin_profile(BuiltinProfile::Simulation, Encoding::Json);
        let gnosis = builtin_profile(BuiltinProfile::Gnosis, Encoding::Json);

        assert_ne!(simulation.tiling_scheme(), gnosis.tiling_scheme());
    }

    // -----------------------------------------------------------------------
    // The rosters the library does not publish
    // -----------------------------------------------------------------------

    /// [`METADATA_ENCODINGS`] holds every variant. The `match` is exhaustive,
    /// so a variant added to [`MetadataEncoding`] fails to compile here rather
    /// than quietly becoming unspellable in a descriptor.
    #[test]
    fn cli_profile_the_encoding_roster_is_whole() {
        fn known(encoding: MetadataEncoding) -> bool {
            match encoding {
                MetadataEncoding::Xml | MetadataEncoding::Json | MetadataEncoding::Gpkg => true,
            }
        }
        assert_eq!(METADATA_ENCODINGS.len(), 3);
        for encoding in METADATA_ENCODINGS {
            assert!(known(encoding), "{encoding:?}");
        }
    }

    /// [`UNITS_OF_MEASURE`] holds every variant, proved the same way.
    #[test]
    fn cli_profile_the_uom_roster_is_whole() {
        fn known(uom: UnitOfMeasure) -> bool {
            match uom {
                UnitOfMeasure::Meters
                | UnitOfMeasure::Feet
                | UnitOfMeasure::Kilometers
                | UnitOfMeasure::Miles => true,
            }
        }
        assert_eq!(UNITS_OF_MEASURE.len(), 4);
        for uom in UNITS_OF_MEASURE {
            assert!(known(uom), "{uom:?}");
        }
    }

    /// **The descriptor cannot fall behind the library.** Every variant of
    /// every vocabulary the descriptor draws on has a spelling the descriptor
    /// accepts, and the spelling round-trips to the variant it names.
    ///
    /// Where the library publishes an `ALL` constant the test iterates it, so
    /// a variant added there arrives here without anyone editing this file.
    /// Where it does not — [`MetadataEncoding`], [`UnitOfMeasure`],
    /// [`StorageTechnology`] — the roster is this module's, and the two
    /// preceding tests hold the first two of those complete. The third cannot
    /// be held complete from outside the library, and that limit is stated on
    /// [`STORAGE_TECHNOLOGIES`] rather than hidden.
    #[test]
    fn cli_profile_the_descriptor_spells_every_library_variant() {
        for rule in CaseRule::ALL {
            assert_eq!(parse_case_rule(&rule.to_string()), Ok(rule), "{rule}");
        }
        for standard in MetadataStandard::ALL {
            assert_eq!(
                parse_metadata_standard(standard.as_str()),
                Ok(standard),
                "{standard:?}"
            );
        }
        for encoding in METADATA_ENCODINGS {
            assert_eq!(
                parse_metadata_encoding(encoding.as_str()),
                Ok(encoding),
                "{encoding:?}"
            );
        }
        for uom in UNITS_OF_MEASURE {
            assert_eq!(parse_uom(uom.as_str()), Ok(uom), "{uom:?}");
        }
        for scheme in TilingSchemeId::ALL {
            assert_eq!(
                parse_tiling_scheme(scheme.as_str()),
                Ok(scheme),
                "{scheme:?}"
            );
        }
        for technology in STORAGE_TECHNOLOGIES {
            assert_eq!(
                parse_storage_technology(technology.as_str()),
                Ok(technology),
                "{technology:?}"
            );
        }
        for &class in RequirementsClass::ALL {
            assert_eq!(
                parse_conformance_classes(&serde_json::json!([class.as_str()])),
                Ok(vec![class]),
                "{class}"
            );
        }
    }

    /// The two keywords cover the two rosters Annex A names, and they are the
    /// library's own slices rather than lists retyped here.
    #[test]
    fn cli_profile_the_class_keywords_are_the_library_rosters() {
        assert_eq!(
            parse_conformance_classes(&serde_json::json!(ALL_CLASSES)),
            Ok(RequirementsClass::ALL.to_vec())
        );
        assert_eq!(
            parse_conformance_classes(&serde_json::json!(MANDATORY_CLASSES)),
            Ok(RequirementsClass::MANDATORY.to_vec())
        );
    }

    /// A vocabulary list reads as a sentence at one, two, and many.
    #[test]
    fn cli_profile_a_vocabulary_reads_as_a_list() {
        assert_eq!(spellings(["a"]), "`a`");
        assert_eq!(spellings(["a", "b"]), "`a` or `b`");
        assert_eq!(spellings(["a", "b", "c"]), "`a`, `b`, or `c`");
    }

    // -----------------------------------------------------------------------
    // The profile the document describes
    // -----------------------------------------------------------------------

    /// A descriptor carrying only its required fields, as JSON text.
    fn minimal_json() -> String {
        serde_json::json!({
            "name": "acme-sim",
            "case_rule": "PascalCase",
            "language": "en",
            "metadata_standard": "DCAT",
            "metadata_encoding": "json",
            "uom": "M",
            "storage_crs_wkt": WGS84_2D_WKT,
        })
        .to_string()
    }

    /// Load `text` as a descriptor named `profile.json` in a fresh directory.
    fn load(text: &str) -> Result<DescriptorProfile, UsageError> {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let path = tmp.path().join("profile.json");
        std::fs::write(&path, text).expect("the descriptor");

        DescriptorProfile::load(&path)
    }

    /// The profile a descriptor that must load describes.
    fn loaded(text: &str) -> DescriptorProfile {
        match load(text) {
            Ok(profile) => profile,
            Err(error) => panic!("the descriptor should load: {error}"),
        }
    }

    /// Every default of design §9.1, asserted through the trait rather than
    /// through the fields — the trait is what `validate` reads.
    #[test]
    fn cli_profile_the_documented_defaults_are_the_ones_applied() {
        let profile = loaded(&minimal_json());

        assert_eq!(profile.name(), "acme-sim");
        assert_eq!(profile.storage_technology(), StorageTechnology::FileSystem);
        assert_eq!(profile.conformance_classes(), RequirementsClass::ALL);
        assert_eq!(profile.tiling_scheme(), None);
        assert_eq!(profile.root_folder_name(), "cdb");
        assert!(profile.known_extensions().is_empty());
        assert!(profile.attribute_model().is_none());
        assert!(profile.is_resource_metadata("/Tiles/metadata/Roads.json"));
        // `reserved_names` defaults empty, but the convention directory is
        // reserved on its own — see the dedicated test below.
        assert!(profile.style_guide().is_reserved("metadata"));
        assert!(!profile.style_guide().is_reserved("Odd_Name"));
    }

    /// The declared `resource_metadata_dir` is reserved from the case rule
    /// automatically, exactly as `SimulationProfile::style_guide` reserves
    /// its own `metadata` (Requirement Name5): a profile that convicted the
    /// layout its own convention field declares would contradict itself.
    /// `reserved_names` stays what it says — the profile's *extra* names.
    #[test]
    fn cli_profile_the_convention_directory_is_reserved_automatically() {
        let mut descriptor: serde_json::Value =
            serde_json::from_str(&minimal_json()).expect("json");
        descriptor["resource_metadata_dir"] = serde_json::json!("Records");

        let guide = loaded(&descriptor.to_string()).style_guide();

        assert!(guide.is_reserved("Records"));
        assert!(
            !guide.is_reserved("metadata"),
            "only the declared directory is reserved, not the default one"
        );
    }

    /// `reserved_names` reaches the style guide, and the library's own
    /// spec-mandated reservations survive alongside it: the descriptor adds
    /// to that set rather than replacing it.
    #[test]
    fn cli_profile_reserved_names_reach_the_style_guide() {
        let mut descriptor: serde_json::Value =
            serde_json::from_str(&minimal_json()).expect("json");
        descriptor["reserved_names"] = serde_json::json!(["metadata", "Odd_Name"]);

        let guide = loaded(&descriptor.to_string()).style_guide();

        assert!(guide.is_reserved("metadata"));
        assert!(guide.is_reserved("Odd_Name"));
        assert!(guide.is_reserved("global_metadata"), "the library's own");
        assert_eq!(guide.case_rule(), CaseRule::PascalCase);
        assert_eq!(guide.language(), "en");
    }

    /// The recognizer follows `resource_metadata_dir`, folds ASCII case on
    /// both the directory and the extension, and matches whole components
    /// only — the convention `SimulationProfile` implements
    /// (`docs/CONFORMANCE.md` §9).
    #[test]
    fn cli_profile_is_resource_metadata_follows_the_simulation_convention() {
        let mut descriptor: serde_json::Value =
            serde_json::from_str(&minimal_json()).expect("json");
        descriptor["resource_metadata_dir"] = serde_json::json!("Records");
        let profile = loaded(&descriptor.to_string());

        for path in [
            "/Tiles/Records/Roads.json",
            "/Tiles/records/Roads.JSON",
            "/Tiles/RECORDS/Roads.xml",
            "Tiles/Records/Roads.gpkg",
        ] {
            assert!(profile.is_resource_metadata(path), "{path}");
        }
        for path in [
            "/Tiles/RecordsArchive/Roads.json",
            "/Tiles/metadata/Roads.json",
            "/Tiles/Records/Roads.tif",
            "/Tiles/Records/Roads",
            "/Records",
        ] {
            assert!(!profile.is_resource_metadata(path), "{path}");
        }
    }

    /// The descriptor's recognizer agrees with `SimulationProfile`'s over the
    /// convention they share. The two are separate implementations of one
    /// rule, and a descriptor mirroring the simulation profile has to be the
    /// simulation profile in this respect too.
    #[test]
    fn cli_profile_is_resource_metadata_agrees_with_the_built_in() {
        let profile = loaded(&minimal_json());
        let built_in = SimulationProfile::json();

        for path in [
            "/Tiles/metadata/Roads.json",
            "/Tiles/Metadata/Roads.JSON",
            "/Tiles/METADATA/Roads.gpkg",
            "/Tiles/MetadataRecords/Roads.json",
            "/global_metadata/global_metadata.json",
            "/Tiles/Roads.gpkg",
            "/Tiles/metadata/Roads",
        ] {
            assert_eq!(
                profile.is_resource_metadata(path),
                built_in.is_resource_metadata(path),
                "{path}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Pre-flight, at the unit level
    // -----------------------------------------------------------------------

    /// The message of a descriptor that must not load.
    fn refused(descriptor: &serde_json::Value) -> String {
        match load(&descriptor.to_string()) {
            Err(error) => error.message,
            Ok(profile) => panic!("the descriptor should be refused, got {}", profile.name()),
        }
    }

    /// A descriptor value with one field overridden.
    fn with(field: &str, value: serde_json::Value) -> serde_json::Value {
        let mut descriptor: serde_json::Value =
            serde_json::from_str(&minimal_json()).expect("json");
        descriptor[field] = value;

        descriptor
    }

    /// Design §5 rule 6: `storage_crs` and `language` both return a `Result`,
    /// and both are exercised before the datastore is opened, so a broken
    /// yardstick is refused rather than charged to a datastore.
    #[test]
    fn req_profile_preflight_exercises_both_fallible_accessors() {
        let crs = refused(&with(
            "storage_crs_wkt",
            serde_json::json!("PROJCRS[\"utm\"]"),
        ));
        assert!(crs.contains("storage_crs_wkt"), "{crs}");

        let language = refused(&with("language", serde_json::json!("no tag")));
        assert!(language.contains("language"), "{language}");
    }

    /// A declared attribute model is validated by the library's own document
    /// check, and the canonical model it returns is the one the profile then
    /// declares — so a model written with stray whitespace matches the
    /// datastore's canonicalized copy instead of convicting it.
    #[test]
    fn req_profile_preflight_canonicalizes_a_declared_attribute_model() {
        let profile = loaded(
            &with(
                "attribute_model",
                serde_json::json!({
                    "attributes": [
                        {"id": " 1 ", "name": " StreetName ", "description": " A street "},
                    ]
                }),
            )
            .to_string(),
        );

        let model = profile.attribute_model().expect("a declared model");
        assert_eq!(model.attributes[0].id, "1");
        assert_eq!(model.attributes[0].name, "StreetName");
        assert_eq!(model.attributes[0].description, "A street");
        assert!(model.validate().is_ok());
    }

    /// `gpkg` is refused at pre-flight, and the message says the refusal is a
    /// stance about this build rather than a typo in the descriptor.
    #[test]
    fn req_profile_preflight_refuses_gpkg_in_its_own_words() {
        let message = refused(&with("metadata_encoding", serde_json::json!("gpkg")));

        for fragment in ["metadata_encoding", "gpkg", "deliberate", "TDD_PLAN §8"] {
            assert!(message.contains(fragment), "{fragment}: {message}");
        }
    }
}
