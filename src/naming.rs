//! Resource Path and File Naming requirements module.
//!
//! Implements the `/req/core/naming-system` requirements class (spec §7.4).
//! SHALL requirements surface as [`NamingViolation`] errors; SHOULD
//! recommendations surface as [`NamingWarning`]s. The two are never
//! conflated — with one deliberate exception: Requirement Name7-B (§7.4.8)
//! is a SHALL, but its predicate ("is this extension industry standard?")
//! is undecidable by this crate, so it too surfaces as a [`NamingWarning`]
//! that a profile can silence by vouching via
//! [`crate::profiles::ApplicationProfile::known_extensions`]. That is the
//! honest handling of an unevaluable SHALL, not a demotion of its force.

use std::collections::BTreeSet;
use std::fmt;

use thiserror::Error;

/// Characters that SHALL NOT be used in any CDB resource path, folder name,
/// or resource name (Recommendation Name1-B, spec §7.4.3). Blank spaces are
/// covered separately by Requirement Name1 (`/req/core/name-spaces`).
pub const FORBIDDEN_CHARACTERS: [char; 17] = [
    '#', '%', '&', '{', '}', '\\', '<', '>', '*', '?', '/', '$', '!', '\'', '"', ':', '@',
];

/// A violation of a SHALL requirement in the naming module.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NamingViolation {
    #[error("empty string is not a valid resource, folder, or path component name")]
    EmptyName,
    #[error("whitespace in name {name:?} (violates /req/core/name-spaces)")]
    ContainsSpace { name: String },
    #[error(
        "forbidden character {character:?} in name {name:?} (violates /req/core/name-unicode B)"
    )]
    ForbiddenCharacter { name: String, character: char },
    /// Not spelled out in the spec's character list, but control characters
    /// are unportable across file systems; treated as a hard error.
    #[error("control character U+{codepoint:04X} in name {name:?} (unportable in file names)")]
    ControlCharacter { name: String, codepoint: u32 },
    #[error(
        "name {name:?} does not follow the {expected} case rule (violates /req/core/name-case)"
    )]
    CaseRuleViolation { name: String, expected: CaseRule },
    #[error("empty component in path {path:?}")]
    EmptyPathComponent { path: String },
    #[error(
        "path {path:?} contains traversal component {component:?}; all content must resolve \
         under the datastore root (violates /req/core/file-cdb-root-location)"
    )]
    PathTraversal { path: String, component: String },
}

/// A naming finding reported as a warning rather than a hard error. Most are
/// SHOULD recommendations; [`NamingWarning::NonSpecExtension`] is the one
/// exception (a SHALL with an undecidable predicate — see its own doc).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NamingWarning {
    /// Recommendation Name1-A (spec §7.4.3): unicode SHOULD not be used.
    NonAscii { name: String },
    /// Requirement Name7-B (spec §7.4.8): industry standard extensions
    /// SHALL be used — a SHALL, not a SHOULD. Its predicate ("is this
    /// extension industry standard?") cannot be checked mechanically, so it
    /// surfaces as a warning a profile can silence by vouching for the
    /// extension, rather than as a hard error the crate cannot honestly
    /// decide.
    NonSpecExtension { name: String, extension: String },
}

impl fmt::Display for NamingWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NamingWarning::NonAscii { name } => write!(
                f,
                "non-ASCII characters in name {name:?} (recommendation /req/core/name-unicode A)"
            ),
            NamingWarning::NonSpecExtension { name, extension } => write!(
                f,
                "extension {extension:?} of {name:?} is not in the spec extension table; it \
                 must be an industry-standard extension (/req/core/name-extensions B)"
            ),
        }
    }
}

/// The four case rules of Requirement Name6 (`/req/core/name-case`, §7.4.7).
/// A CDB datastore uses exactly one rule for all names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaseRule {
    PascalCase,
    CamelCase,
    /// Lowercase words joined by `_`; the spec also admits an all-caps form.
    SnakeCase,
    KebabCase,
}

impl CaseRule {
    pub const ALL: [CaseRule; 4] = [
        CaseRule::PascalCase,
        CaseRule::CamelCase,
        CaseRule::SnakeCase,
        CaseRule::KebabCase,
    ];

    /// Whether `stem` (a name without its file extension) satisfies this rule.
    ///
    /// Letter case is checked Unicode-aware so that non-ASCII names stay a
    /// SHOULD-level concern (Recommendation Name1-A) rather than becoming a
    /// case violation. Names without letters (e.g. `"001"`) match every rule.
    pub fn matches(self, stem: &str) -> bool {
        fn segments_nonempty(s: &str, sep: char) -> bool {
            !s.split(sep).any(str::is_empty)
        }
        fn first_letter(s: &str) -> Option<char> {
            s.chars().find(|c| c.is_alphabetic())
        }
        fn letters_all(s: &str, pred: fn(char) -> bool) -> bool {
            s.chars().filter(|c| c.is_alphabetic()).all(pred)
        }
        match self {
            CaseRule::PascalCase => {
                stem.chars().all(char::is_alphanumeric)
                    && first_letter(stem).is_none_or(char::is_uppercase)
            }
            CaseRule::CamelCase => {
                stem.chars().all(char::is_alphanumeric)
                    && first_letter(stem).is_none_or(char::is_lowercase)
            }
            CaseRule::SnakeCase => {
                stem.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && segments_nonempty(stem, '_')
                    && (letters_all(stem, char::is_lowercase)
                        || letters_all(stem, char::is_uppercase))
            }
            CaseRule::KebabCase => {
                stem.chars().all(|c| c.is_alphanumeric() || c == '-')
                    && segments_nonempty(stem, '-')
                    && letters_all(stem, char::is_lowercase)
            }
        }
    }

    /// Whether a file name satisfies this rule; the extension is exempt.
    pub fn matches_file_name(self, name: &str) -> bool {
        self.matches(split_extension(name).0)
    }

    /// All rules a name is consistent with, in `ALL` order.
    pub fn classify(name: &str) -> Vec<CaseRule> {
        CaseRule::ALL
            .into_iter()
            .filter(|rule| rule.matches(name))
            .collect()
    }
}

impl fmt::Display for CaseRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The spec's own spellings (§7.4.7).
        f.write_str(match self {
            CaseRule::PascalCase => "PascalCase",
            CaseRule::CamelCase => "camelCase",
            CaseRule::SnakeCase => "Snake_case",
            CaseRule::KebabCase => "kebab-case",
        })
    }
}

/// Structural validation of a single folder or resource name
/// (Requirements Name1, Name1-B; not case-rule aware).
pub fn validate_component(name: &str) -> Result<(), NamingViolation> {
    if name.is_empty() {
        return Err(NamingViolation::EmptyName);
    }
    for character in name.chars() {
        if character.is_whitespace() {
            return Err(NamingViolation::ContainsSpace {
                name: name.to_owned(),
            });
        }
        if FORBIDDEN_CHARACTERS.contains(&character) {
            return Err(NamingViolation::ForbiddenCharacter {
                name: name.to_owned(),
                character,
            });
        }
        if character.is_control() {
            return Err(NamingViolation::ControlCharacter {
                name: name.to_owned(),
                codepoint: character as u32,
            });
        }
    }
    Ok(())
}

/// SHOULD-level findings for a single name (Recommendation Name1-A).
pub fn component_warnings(name: &str) -> Vec<NamingWarning> {
    let mut warnings = Vec::new();
    if !name.is_ascii() {
        warnings.push(NamingWarning::NonAscii {
            name: name.to_owned(),
        });
    }
    warnings
}

/// Structural validation of a `/`-separated path. A single leading `/` is
/// allowed (the datastore root, Requirement File5). Rejects empty components
/// and `.`/`..` traversal (Requirement File2: all content under the root).
pub fn validate_path(path: &str) -> Result<(), NamingViolation> {
    if path.is_empty() {
        return Err(NamingViolation::EmptyName);
    }
    let relative = path.strip_prefix('/').unwrap_or(path);
    if relative.is_empty() {
        return Ok(()); // "/" is the datastore root itself.
    }
    for component in relative.split('/') {
        if component.is_empty() {
            return Err(NamingViolation::EmptyPathComponent {
                path: path.to_owned(),
            });
        }
        if component == "." || component == ".." {
            return Err(NamingViolation::PathTraversal {
                path: path.to_owned(),
                component: component.to_owned(),
            });
        }
        validate_component(component)?;
    }
    Ok(())
}

/// Splits `name` into (stem, extension) at the last dot. A leading dot is
/// part of the stem, not an extension separator.
pub fn split_extension(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(index) if index > 0 => (&name[..index], Some(&name[index + 1..])),
        _ => (name, None),
    }
}

/// Looks up an extension (case-insensitive) in the Requirement Name7 table
/// (§7.4.8), returning the file format it denotes.
pub fn known_extension(extension: &str) -> Option<&'static str> {
    let extension = extension.to_ascii_lowercase();
    Some(match extension.as_str() {
        "bmp" => "Bitmap Image",
        "dbf" | "dbt" => "dBASE",
        "gpkg" => "GeoPackage",
        "gltf" => "glTF JSON/ASCII",
        "glb" => "glTF Binary",
        "jp2" => "JPEG 2000",
        "json" => "JSON",
        "flt" => "OpenFlight",
        "shp" | "shx" => "Shapefile",
        "rgb" => "SGI Image",
        "rgba" => "SGI Image + Alpha",
        "tif" => "TIFF",
        "xml" | "xsd" => "XML",
        "zip" => "ZIP",
        _ => return None,
    })
}

/// Warning-level findings for a file name: non-ASCII (Recommendation
/// Name1-A, a SHOULD) and extensions outside the Name7 table (Requirement
/// Name7-B, a SHALL with an undecidable predicate — see
/// [`NamingWarning::NonSpecExtension`]).
pub fn file_warnings(name: &str) -> Vec<NamingWarning> {
    let mut warnings = component_warnings(name);
    if let (_, Some(extension)) = split_extension(name)
        && known_extension(extension).is_none()
    {
        warnings.push(NamingWarning::NonSpecExtension {
            name: name.to_owned(),
            extension: extension.to_owned(),
        });
    }
    warnings
}

/// The crate-wide case stance in one comparison: **guards fold, requirements
/// don't.**
///
/// This is the *guard* half — ASCII case-insensitive equality for every
/// comparison that decides **which on-disk thing a name points at**: the
/// reserved-tree fences ([`crate::versioning`]'s `versions/` journal and the
/// `global_metadata/` records), reserved-stem detection, convention-directory
/// recognition, and root-folder matching. On a case-insensitive,
/// case-preserving filesystem (APFS, NTFS) `/Versions/x` and `/versions/x`
/// name the same bytes, so a byte-exact fence is no fence at all there;
/// catching a mis-cased spelling costs nothing, missing it is a hole. The
/// accepted cost is the mirror image: on a genuinely case-sensitive volume a
/// real `Versions/` directory is refused as an asset target — which
/// Requirement Name6 would flag anyway.
///
/// The *requirement* half is deliberately **not** folded, and this function
/// must not be used for it: Requirement Name6's case rule
/// ([`CaseRule::matches`], and the [`StyleGuide::is_reserved`] exemption that
/// gates it) is *about* case, so folding it would make it vacuous;
/// Requirement Attr1-C's literal file name
/// ([`crate::attribution::parse_file_name`]) is a literal mandate;
/// attribute-id uniqueness is byte-exact after trimming (Requirement Attr2);
/// and metadata element values are data, not paths.
///
/// Folding is ASCII-only. Full Unicode case folding needs tables this crate
/// has no business carrying, and CDB names are ASCII by construction
/// (Recommendation Name1-B, §7.4.3, which forbids the punctuation that would
/// otherwise invite non-ASCII spellings and which this module warns about).
pub(crate) fn guard_eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The naming style guide an application profile SHALL define
/// (Requirement Name5, `/req/core/name-ap-guide`, §7.4.6). Carries the
/// datastore-wide case rule (Name6) and language (Name3).
///
/// Names mandated verbatim by the spec (e.g. `global_metadata`, Requirement
/// File6; `vector_attributes`, Requirement Attr1-C, realized by
/// [`crate::attribution::VECTOR_ATTRIBUTES_STEM`]) — or by this crate's
/// persistence of spec-mandated records (`crs.wkt`, Requirement CRS5, written
/// by [`crate::crs::StorageCrs::write_to`]; the `versions` journal dir,
/// Requirement V1 §7.14.2, written by the versioning facade) — are reserved and exempt from
/// the case rule, which would otherwise conflict with them.
#[derive(Debug, Clone)]
pub struct StyleGuide {
    case_rule: CaseRule,
    language: String,
    reserved_names: BTreeSet<String>,
}

impl StyleGuide {
    pub fn new(case_rule: CaseRule, language: impl Into<String>) -> Self {
        // "crs" is reserved for every profile: this crate's own
        // `StorageCrs::write_to` persists the storage CRS as `crs.wkt`
        // (Requirement CRS5), so per-profile reservation would be a footgun.
        let reserved_names = ["global_metadata", "vector_attributes", "crs", "versions"]
            .into_iter()
            .map(String::from)
            .collect();
        Self {
            case_rule,
            language: language.into(),
            reserved_names,
        }
    }

    pub fn case_rule(&self) -> CaseRule {
        self.case_rule
    }

    /// The single language for all names (Requirement Name3-A); SHOULD be
    /// English (Name3-B). Tag syntax is validated by the metadata module
    /// (Requirement Metadata4, BCP 47).
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Marks a name (stem) as spec-mandated, exempting it from the case rule.
    pub fn reserve_name(&mut self, name: impl Into<String>) {
        self.reserved_names.insert(name.into());
    }

    /// Whether `stem` is spec-mandated and therefore exempt from the Name6
    /// case rule. Byte-exact, deliberately: this gate is the *requirement*
    /// half of the crate's case stance (see the crate-internal `guard_eq`), and
    /// exempt `Global_Metadata` too — making Name6 vacuous for precisely the
    /// names the spec mandates verbatim, which is the opposite of what the
    /// exemption is for. A mis-cased reserved name is ordinary content as far
    /// as this check goes, and is judged by the case rule like any other.
    pub fn is_reserved(&self, stem: &str) -> bool {
        self.reserved_names.contains(stem)
    }

    /// Structural checks plus the case rule applied to the stem.
    pub fn validate_component(&self, name: &str) -> Result<(), NamingViolation> {
        validate_component(name)?;
        let (stem, _) = split_extension(name);
        if self.is_reserved(stem) {
            return Ok(());
        }
        if !self.case_rule.matches(stem) {
            return Err(NamingViolation::CaseRuleViolation {
                name: name.to_owned(),
                expected: self.case_rule,
            });
        }
        Ok(())
    }

    /// Validates every component of a path against this style guide.
    pub fn validate_path(&self, path: &str) -> Result<(), NamingViolation> {
        validate_path(path)?;
        let relative = path.strip_prefix('/').unwrap_or(path);
        if relative.is_empty() {
            return Ok(());
        }
        for component in relative.split('/') {
            self.validate_component(component)?;
        }
        Ok(())
    }
}

impl Default for StyleGuide {
    fn default() -> Self {
        Self::new(CaseRule::PascalCase, "en")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guard half of the crate's case stance — [`guard_eq`] folds ASCII
    /// case for path guards, and folds **ASCII only**: a `to_lowercase()`
    /// implementation would also fold `É`/`é`, dragging in Unicode case
    /// tables this crate deliberately does not carry.
    #[test]
    fn req_core_name_guard_eq_folds_ascii_case_only() {
        for (a, b) in [
            ("Versions", "versions"),
            ("GLOBAL_METADATA", "global_metadata"),
            ("Vector_Attributes", "vector_attributes"),
            ("CDB", "cdb"),
            ("MeTaDaTa", "metadata"),
        ] {
            assert!(guard_eq(a, b), "{a} vs {b}");
        }
        assert!(!guard_eq("versionz", "versions"));
        assert!(!guard_eq("versions2", "versions"));
        // ASCII-only, asserted rather than assumed.
        assert!(!guard_eq("caf\u{c9}", "caf\u{e9}"));
    }

    /// §7.4.7 Requirement Name6 — the *requirement* half of the split rule.
    /// The case rule itself, and the reserved-name exemption that gates it,
    /// stay byte-exact: folding either would make Name6 vacuous for exactly
    /// the names the spec mandates verbatim, so a `Global_Metadata` folder
    /// would escape unremarked instead of drawing a case finding.
    #[test]
    fn req_core_name_case_rule_is_not_folded() {
        assert!(!CaseRule::PascalCase.matches("roadNetwork"));
        assert!(!CaseRule::SnakeCase.matches("Road_Network"));
        let guide = StyleGuide::new(CaseRule::PascalCase, "en");
        assert!(guide.is_reserved("global_metadata"));
        assert!(!guide.is_reserved("Global_Metadata"));
        assert!(matches!(
            guide.validate_component("Global_Metadata.json"),
            Err(NamingViolation::CaseRuleViolation { .. })
        ));
    }

    /// §7.4.2 Requirement Name1 `/req/core/name-spaces`.
    #[test]
    fn req_core_name_spaces_rejects_space() {
        assert_eq!(
            validate_component("road network"),
            Err(NamingViolation::ContainsSpace {
                name: "road network".into()
            })
        );
    }

    /// §7.4.2 — "blank spaces" covers all whitespace.
    #[test]
    fn req_core_name_spaces_rejects_tab_and_newline() {
        for name in ["road\tnetwork", "road\nnetwork"] {
            assert!(
                matches!(
                    validate_component(name),
                    Err(NamingViolation::ContainsSpace { .. })
                ),
                "{name:?}"
            );
        }
    }

    #[test]
    fn req_core_name_spaces_accepts_separator_names() {
        for name in [
            "road_network",
            "RoadNetwork",
            "road-network",
            "Materials.xml",
        ] {
            assert_eq!(validate_component(name), Ok(()), "{name:?}");
        }
    }

    /// §7.4.3 Recommendation Name1-B — every listed character is rejected.
    #[test]
    fn req_core_name_unicode_rejects_each_forbidden_character() {
        for &ch in FORBIDDEN_CHARACTERS.iter() {
            let name = format!("road{ch}network");
            match validate_component(&name) {
                Err(NamingViolation::ForbiddenCharacter { character, .. }) => {
                    assert_eq!(character, ch);
                }
                other => panic!("{name:?}: expected ForbiddenCharacter, got {other:?}"),
            }
        }
    }

    #[test]
    fn req_core_name_unicode_component_rejects_forward_slash() {
        assert!(matches!(
            validate_component("a/b"),
            Err(NamingViolation::ForbiddenCharacter { character: '/', .. })
        ));
    }

    #[test]
    fn control_characters_are_rejected_for_portability() {
        assert!(matches!(
            validate_component("road\u{0}net"),
            Err(NamingViolation::ControlCharacter { .. })
        ));
    }

    #[test]
    fn empty_name_is_rejected() {
        assert_eq!(validate_component(""), Err(NamingViolation::EmptyName));
    }

    /// §7.4.3 Recommendation Name1-A — unicode SHOULD not be used:
    /// a warning, not a violation.
    #[test]
    fn rec_core_name_unicode_warns_on_non_ascii_but_validates() {
        assert_eq!(validate_component("café"), Ok(()));
        assert_eq!(
            component_warnings("café"),
            vec![NamingWarning::NonAscii {
                name: "café".into()
            }]
        );
        assert!(component_warnings("cafe").is_empty());
    }

    /// §7.4.7 Requirement Name6 `/req/core/name-case`.
    #[test]
    fn req_core_name_case_pascal() {
        assert!(CaseRule::PascalCase.matches("RoadNetwork"));
        assert!(CaseRule::PascalCase.matches("CDB"));
        assert!(CaseRule::PascalCase.matches("Tile01"));
        assert!(!CaseRule::PascalCase.matches("roadNetwork"));
        assert!(!CaseRule::PascalCase.matches("road_network"));
        assert!(!CaseRule::PascalCase.matches("Road-Network"));
    }

    #[test]
    fn req_core_name_case_camel() {
        assert!(CaseRule::CamelCase.matches("roadNetwork"));
        assert!(CaseRule::CamelCase.matches("elevation"));
        assert!(!CaseRule::CamelCase.matches("RoadNetwork"));
        assert!(!CaseRule::CamelCase.matches("road_network"));
    }

    #[test]
    fn req_core_name_case_snake_incl_all_caps_variant() {
        assert!(CaseRule::SnakeCase.matches("road_network"));
        assert!(CaseRule::SnakeCase.matches("ROAD_NETWORK"));
        assert!(CaseRule::SnakeCase.matches("lod_0"));
        assert!(!CaseRule::SnakeCase.matches("Road_Network"));
        assert!(!CaseRule::SnakeCase.matches("road__network"));
        assert!(!CaseRule::SnakeCase.matches("_road"));
        assert!(!CaseRule::SnakeCase.matches("road-network"));
    }

    #[test]
    fn req_core_name_case_kebab() {
        assert!(CaseRule::KebabCase.matches("road-network"));
        assert!(CaseRule::KebabCase.matches("lod-0"));
        assert!(!CaseRule::KebabCase.matches("Road-Network"));
        assert!(!CaseRule::KebabCase.matches("road_network"));
        assert!(!CaseRule::KebabCase.matches("road--network"));
        assert!(!CaseRule::KebabCase.matches("-road"));
    }

    #[test]
    fn req_core_name_case_digit_only_names_match_any_rule() {
        for rule in CaseRule::ALL {
            assert!(rule.matches("001"), "{rule}");
        }
    }

    #[test]
    fn req_core_name_case_classify() {
        assert_eq!(
            CaseRule::classify("roads"),
            vec![
                CaseRule::CamelCase,
                CaseRule::SnakeCase,
                CaseRule::KebabCase
            ]
        );
        assert_eq!(CaseRule::classify("Roads"), vec![CaseRule::PascalCase]);
        assert_eq!(CaseRule::classify("Road Network"), Vec::<CaseRule>::new());
    }

    #[test]
    fn req_core_name_case_applies_to_stem_not_extension() {
        assert!(CaseRule::PascalCase.matches_file_name("RoadNetwork.shp"));
        assert!(!CaseRule::PascalCase.matches_file_name("road_network.shp"));
    }

    #[test]
    fn req_core_name_case_display_uses_spec_spellings() {
        assert_eq!(CaseRule::PascalCase.to_string(), "PascalCase");
        assert_eq!(CaseRule::CamelCase.to_string(), "camelCase");
        assert_eq!(CaseRule::SnakeCase.to_string(), "Snake_case");
        assert_eq!(CaseRule::KebabCase.to_string(), "kebab-case");
    }

    /// §7.4.6 Requirement Name5 — the style guide a profile SHALL define,
    /// carrying the Name6 case rule and the Name3 language.
    #[test]
    fn req_core_name_ap_guide_style_guide_enforces_case_and_structure() {
        let guide = StyleGuide::new(CaseRule::PascalCase, "en");
        assert_eq!(guide.case_rule(), CaseRule::PascalCase);
        assert_eq!(guide.language(), "en");
        assert!(guide.validate_component("RoadNetwork.shp").is_ok());
        assert!(matches!(
            guide.validate_component("road network"),
            Err(NamingViolation::ContainsSpace { .. })
        ));
        assert!(matches!(
            guide.validate_component("road_network"),
            Err(NamingViolation::CaseRuleViolation { .. })
        ));
        assert_eq!(StyleGuide::default().case_rule(), CaseRule::PascalCase);
    }

    /// Names the spec mandates verbatim are exempt from the case rule
    /// (File6 `global_metadata`, Attr1-C `vector_attributes.<ext>`).
    #[test]
    fn req_core_name_ap_guide_spec_mandated_names_are_exempt() {
        let guide = StyleGuide::new(CaseRule::PascalCase, "en");
        assert!(guide.validate_component("global_metadata").is_ok());
        assert!(guide.validate_component("vector_attributes.json").is_ok());
    }

    /// §7.4.7 — a datastore mixing case rules is detected.
    #[test]
    fn req_core_name_case_mixed_datastore_detected() {
        let guide = StyleGuide::new(CaseRule::PascalCase, "en");
        let names = ["RoadNetwork", "Elevation", "road_network"];
        let violations: Vec<_> = names
            .iter()
            .filter_map(|n| guide.validate_component(n).err())
            .collect();
        assert_eq!(violations.len(), 1);
    }

    /// §7.4.8 Requirement Name7 `/req/core/name-extensions`.
    #[test]
    fn req_core_name_extensions_spec_table_is_recognized() {
        for ext in [
            "bmp", "dbf", "dbt", "gpkg", "gltf", "glb", "jp2", "json", "flt", "shp", "shx", "rgb",
            "rgba", "tif", "xml", "xsd", "zip",
        ] {
            assert!(known_extension(ext).is_some(), "{ext}");
        }
        assert_eq!(known_extension("shp"), Some("Shapefile"));
        assert_eq!(known_extension("TIF"), Some("TIFF"));
        assert_eq!(known_extension("png"), None);
    }

    #[test]
    fn req_core_name_extensions_unknown_extension_warns() {
        assert_eq!(
            file_warnings("overview.png"),
            vec![NamingWarning::NonSpecExtension {
                name: "overview.png".into(),
                extension: "png".into(),
            }]
        );
        assert!(file_warnings("Materials.xml").is_empty());
        assert!(file_warnings("extensionless").is_empty());
    }

    #[test]
    fn validate_path_accepts_absolute_and_relative() {
        assert_eq!(validate_path("cdb/Tiles/RoadNetwork.shp"), Ok(()));
        assert_eq!(validate_path("/cdb/Tiles"), Ok(()));
        assert_eq!(validate_path("/"), Ok(()));
    }

    #[test]
    fn validate_path_rejects_empty_components() {
        assert!(matches!(
            validate_path("cdb//Tiles"),
            Err(NamingViolation::EmptyPathComponent { .. })
        ));
        assert!(matches!(
            validate_path("cdb/Tiles/"),
            Err(NamingViolation::EmptyPathComponent { .. })
        ));
    }

    /// §7.5.3 Requirement File2 — content must stay under the root.
    #[test]
    fn req_core_file_cdb_root_location_path_traversal_rejected() {
        assert!(matches!(
            validate_path("cdb/../etc"),
            Err(NamingViolation::PathTraversal { .. })
        ));
        assert!(matches!(
            validate_path("./cdb"),
            Err(NamingViolation::PathTraversal { .. })
        ));
    }

    #[test]
    fn validate_path_surfaces_component_violations() {
        assert!(matches!(
            validate_path("a b/c"),
            Err(NamingViolation::ContainsSpace { .. })
        ));
    }

    #[test]
    fn naming_violation_converts_to_cdb_error() {
        let e: crate::CdbError = NamingViolation::EmptyName.into();
        assert!(matches!(e, crate::CdbError::Naming(_)));
    }

    /// §7.4.6 Requirement Name5, with §7.3.1.4 CRS5 and §7.5.7 File6 — the
    /// default style guide reserves the names this crate persists for
    /// spec-mandated records (`crs.wkt` for the storage CRS, `global_metadata`
    /// for the global record), so they are exempt from the case rule.
    #[test]
    fn req_core_name_ap_guide_crate_persistence_names_reserved() {
        let guide = StyleGuide::new(CaseRule::PascalCase, "en");
        assert!(guide.validate_component("crs.wkt").is_ok());
        assert!(guide.validate_component("global_metadata.json").is_ok());
        assert!(StyleGuide::default().validate_component("crs.wkt").is_ok());
    }
}
