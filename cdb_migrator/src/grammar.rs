//! The CDB 1.x name grammar of OGC 15-113r6 §8.6.2–8.6.3: geocell
//! directories, dataset directories, LOD and UREF directories, and the
//! tiled-dataset file naming convention. Parsing accepts ASCII case variants;
//! formatting reconstructs canonical grammar tokens while retaining arbitrary
//! dataset labels verbatim. Callers that need original-byte provenance
//! preserve the raw source path separately.

/// Table 3-29 as (exclusive upper |latitude| bound, zone width in degrees),
/// ascending. Value-identical to `opencdb`'s CDB1GlobalGrid bands.
const ZONE_BANDS: [(i16, u16); 6] = [(50, 1), (70, 2), (75, 3), (80, 4), (89, 6), (90, 12)];

/// Applies the migration's complete default renaming rule: ASCII lowercase.
pub fn fold(name: &str) -> String {
    name.to_ascii_lowercase()
}

/// A geocell named by its southwest corner (Requirements 65/66).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeocellId {
    /// Southwest latitude, −90..=89.
    pub lat_sw: i8,
    /// Southwest longitude, −180..=179, aligned to the zone width.
    pub lon_sw: i16,
}

impl GeocellId {
    /// Parses latitude and longitude directory components.
    ///
    /// Prefixes are ASCII case-insensitive so folded target paths can be
    /// checked against the same address. Numeric fields remain fixed-width
    /// ASCII digits, and formatting always returns the canonical uppercase
    /// spelling.
    pub fn from_dirs(lat_dir: &str, lon_dir: &str) -> Result<Self, String> {
        let lat = parse_component(lat_dir, 2, b'N', b'S', "latitude")?;
        let lon = parse_component(lon_dir, 3, b'E', b'W', "longitude")?;

        if lat_dir.as_bytes()[0].eq_ignore_ascii_case(&b'S') && lat == 0 {
            return Err(format!(
                "latitude directory {lat_dir:?} uses noncanonical negative zero"
            ));
        }
        if lon_dir.as_bytes()[0].eq_ignore_ascii_case(&b'W') && lon == 0 {
            return Err(format!(
                "longitude directory {lon_dir:?} uses noncanonical negative zero"
            ));
        }

        let lat = if lat_dir.as_bytes()[0].eq_ignore_ascii_case(&b'S') {
            -lat
        } else {
            lat
        };
        let lon = if lon_dir.as_bytes()[0].eq_ignore_ascii_case(&b'W') {
            -lon
        } else {
            lon
        };
        let lat_sw =
            i8::try_from(lat).map_err(|_| format!("latitude {lat} out of range -90..=89"))?;
        let cell = Self {
            lat_sw,
            lon_sw: lon,
        };
        cell.validate()?;
        Ok(cell)
    }

    /// Validates values, including instances built directly through the
    /// public fields.
    pub fn validate(&self) -> Result<(), String> {
        if !(-90..=89).contains(&self.lat_sw) {
            return Err(format!("latitude {} out of range -90..=89", self.lat_sw));
        }
        if !(-180..=179).contains(&self.lon_sw) {
            return Err(format!("longitude {} out of range -180..=179", self.lon_sw));
        }

        let width = i32::from(self.zone_width());
        if i32::from(self.lon_sw).rem_euclid(width) != 0 {
            return Err(format!(
                "longitude {} is not aligned to the {width}-degree zone at latitude {}",
                self.lon_sw, self.lat_sw
            ));
        }
        Ok(())
    }

    /// Returns the Table 3-29 zone width for the cell's latitude.
    ///
    /// The cell spans `[lat_sw, lat_sw + 1)`, so southern cells classify by
    /// their equatorward edge: cell `[−70, −69)` remains in the 2° band. The
    /// calculation widens first so direct construction with any `i8` value
    /// cannot overflow.
    pub fn zone_width(&self) -> u16 {
        let lat = i16::from(self.lat_sw);
        let equatorward_magnitude = if lat >= 0 { lat } else { -lat - 1 };
        for (bound, width) in ZONE_BANDS {
            if equatorward_magnitude < bound {
                return width;
            }
        }
        12
    }

    /// Formats the canonical latitude directory name.
    pub fn lat_dir_name(&self) -> String {
        let lat = i16::from(self.lat_sw);
        if lat >= 0 {
            format!("N{lat:02}")
        } else {
            format!("S{:02}", lat.unsigned_abs())
        }
    }

    /// Formats the canonical longitude directory name.
    pub fn lon_dir_name(&self) -> String {
        let lon = i32::from(self.lon_sw);
        if lon >= 0 {
            format!("E{lon:03}")
        } else {
            format!("W{:03}", lon.unsigned_abs())
        }
    }
}

fn parse_component(
    name: &str,
    digit_count: usize,
    positive_prefix: u8,
    negative_prefix: u8,
    kind: &str,
) -> Result<i16, String> {
    let bytes = name.as_bytes();
    if bytes.len() != digit_count + 1
        || !(bytes[0].eq_ignore_ascii_case(&positive_prefix)
            || bytes[0].eq_ignore_ascii_case(&negative_prefix))
    {
        return Err(format!(
            "{kind} directory {name:?} does not have the required prefix and width"
        ));
    }
    let digits = &bytes[1..];
    if !digits.iter().all(u8::is_ascii_digit) {
        return Err(format!("bad ASCII {kind} digits in {name:?}"));
    }

    Ok(digits
        .iter()
        .fold(0_i16, |value, digit| value * 10 + i16::from(*digit - b'0')))
}

/// A CDB 1.x Level of Detail, −10..=23 (§8.6.2.4 and the LOD designation
/// rule: `L` + 2 digits, `C` in lieu of the minus sign).
///
/// The numeric field is public for downstream address conversion. Parsers
/// validate the supported −10..=23 range; direct construction remains
/// representable and formatting stays defined across the full `i8` domain.
///
/// ```
/// use cdb_migrator::grammar::Lod1x;
///
/// let lod = Lod1x(-5);
/// let Lod1x(value) = lod;
/// assert_eq!(value, -5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lod1x(pub i8);

impl Lod1x {
    /// Parses an `L00`..`L23` or `LC01`..`LC10` token.
    pub fn parse(token: &str) -> Result<Self, String> {
        let upper = token.to_ascii_uppercase();
        if let Some(digits) = upper.strip_prefix("LC") {
            if digits.len() != 2 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(format!("LOD token {token:?}: LC needs 2 ASCII digits"));
            }
            let magnitude = parse_ascii_u32(digits, "negative LOD")?;
            if !(1..=10).contains(&magnitude) {
                return Err(format!("negative LOD {token:?} out of LC01..LC10"));
            }
            return Ok(Self(-(magnitude as i8)));
        }
        if let Some(digits) = upper.strip_prefix('L') {
            if digits.len() != 2 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(format!("LOD token {token:?}: L needs 2 ASCII digits"));
            }
            let value = parse_ascii_u32(digits, "LOD")?;
            if value > 23 {
                return Err(format!("LOD {token:?} out of L00..L23"));
            }
            return Ok(Self(value as i8));
        }
        Err(format!("LOD token {token:?} is not Lxx/LCxx"))
    }

    /// Returns the signed numeric LOD.
    pub fn value(&self) -> i8 {
        self.0
    }

    /// Returns the directory holding this LOD: `Lxx`, or `LC` for negatives.
    pub fn dir_name(&self) -> String {
        if self.0 < 0 {
            "LC".to_string()
        } else {
            format!("L{:02}", self.0)
        }
    }

    /// Formats the file-name token as `Lxx` or `LCxx`. Parsed values are in
    /// the canonical ranges; directly constructed values remain safely
    /// representable.
    pub fn token(&self) -> String {
        let value = i16::from(self.0);
        if value < 0 {
            format!("LC{:02}", value.unsigned_abs())
        } else {
            format!("L{value:02}")
        }
    }
}

/// A CDB 1.x dataset directory named `nnn_Name` (§8.6.2.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetDir {
    /// Three-digit dataset code.
    pub code: u16,
    /// Non-empty dataset name following the underscore.
    pub name: String,
}

impl DatasetDir {
    /// Parses a dataset directory name.
    pub fn parse(dir: &str) -> Result<Self, String> {
        let (digits, name) = dir
            .split_once('_')
            .ok_or_else(|| format!("dataset directory {dir:?} has no underscore"))?;
        if digits.len() != 3 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(format!(
                "dataset directory {dir:?} needs a 3-digit ASCII code"
            ));
        }
        if name.is_empty() {
            return Err(format!("dataset directory {dir:?} has an empty name"));
        }
        let code = parse_ascii_u32(digits, "dataset code")? as u16;
        Ok(Self {
            code,
            name: name.to_string(),
        })
    }

    /// Formats the code to three digits and retains the semantic label
    /// verbatim; CDB 1.x does not define a registry from which to recover its
    /// original case.
    pub fn dir_name(&self) -> String {
        format!("{:03}_{}", self.code, self.name)
    }
}

/// A CDB 1.x Requirement 67 tiled-dataset file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileFileName {
    /// Geocell encoded by the leading latitude/longitude token.
    pub geocell: GeocellId,
    /// Dataset code from `Dnnn`.
    pub dataset: u16,
    /// First component selector from `Snnn`.
    pub cs1: u16,
    /// Second component selector from `Tnnn`.
    pub cs2: u16,
    /// Level of detail.
    pub lod: Lod1x,
    /// Variable-width U reference.
    pub uref: u32,
    /// Variable-width R reference.
    pub rref: u32,
    /// Lowercase ASCII file extension without the dot.
    pub extension: String,
}

impl TileFileName {
    /// Parses `LatLon_Dnnn_Snnn_Tnnn_LOD_Un_Rn.<ext>` case-insensitively.
    pub fn parse(file: &str) -> Result<Self, String> {
        let (stem, extension) = file
            .rsplit_once('.')
            .ok_or_else(|| format!("tile file {file:?} has no extension"))?;
        if extension.is_empty()
            || !extension.is_ascii()
            || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(format!(
                "tile file {file:?} needs a non-empty ASCII alphanumeric extension"
            ));
        }

        let mut parts = stem.split('_');
        let latlon = parts
            .next()
            .ok_or_else(|| format!("tile file {file:?} has no geocell field"))?;
        let (latitude, longitude) = match (latlon.get(..3), latlon.get(3..)) {
            (Some(latitude), Some(longitude)) if latlon.len() == 7 => (latitude, longitude),
            _ => {
                return Err(format!(
                    "geocell field {latlon:?} is not an ASCII-safe LatLon token"
                ));
            }
        };
        let geocell = GeocellId::from_dirs(latitude, longitude)?;
        let dataset = parse_fixed_field(parts.next(), b'D', 3)? as u16;
        let cs1 = parse_fixed_field(parts.next(), b'S', 3)? as u16;
        let cs2 = parse_fixed_field(parts.next(), b'T', 3)? as u16;
        let lod = Lod1x::parse(
            parts
                .next()
                .ok_or_else(|| "missing LOD field".to_string())?,
        )?;
        let uref = parse_variable_field(parts.next(), b'U')?;
        let rref = parse_variable_field(parts.next(), b'R')?;
        if parts.next().is_some() {
            return Err(format!("tile file {file:?} has trailing fields"));
        }

        Ok(Self {
            geocell,
            dataset,
            cs1,
            cs2,
            lod,
            uref,
            rref,
            extension: extension.to_ascii_lowercase(),
        })
    }

    /// Reconstructs the canonical spec spelling.
    pub fn canonical(&self) -> String {
        format!(
            "{}{}_D{:03}_S{:03}_T{:03}_{}_U{}_R{}.{}",
            self.geocell.lat_dir_name(),
            self.geocell.lon_dir_name(),
            self.dataset,
            self.cs1,
            self.cs2,
            self.lod.token(),
            self.uref,
            self.rref,
            self.extension
        )
    }
}

fn parse_fixed_field(part: Option<&str>, prefix: u8, digits: usize) -> Result<u32, String> {
    let prefix_char = char::from(prefix);
    let part = part.ok_or_else(|| format!("missing {prefix_char} field"))?;
    let body = ascii_prefixed_body(part, prefix)
        .ok_or_else(|| format!("field {part:?} does not start with {prefix_char}"))?;
    if body.len() != digits || !body.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!(
            "field {part:?} is not {prefix_char}+{digits} ASCII digits"
        ));
    }
    parse_ascii_u32(body, &format!("{prefix_char} field"))
}

fn parse_variable_field(part: Option<&str>, prefix: u8) -> Result<u32, String> {
    let prefix_char = char::from(prefix);
    let part = part.ok_or_else(|| format!("missing {prefix_char} field"))?;
    let body = ascii_prefixed_body(part, prefix)
        .ok_or_else(|| format!("field {part:?} does not start with {prefix_char}"))?;
    if body.is_empty() || !body.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("field {part:?} is not {prefix_char}+ASCII digits"));
    }
    parse_ascii_u32(body, &format!("{prefix_char} field"))
}

fn ascii_prefixed_body(value: &str, prefix: u8) -> Option<&str> {
    let bytes = value.as_bytes();
    if bytes
        .first()
        .is_some_and(|first| first.eq_ignore_ascii_case(&prefix))
    {
        value.get(1..)
    } else {
        None
    }
}

fn parse_ascii_u32(digits: &str, field: &str) -> Result<u32, String> {
    digits
        .parse()
        .map_err(|_| format!("{field} {digits:?} overflows"))
}

#[cfg(test)]
mod tests {
    use super::{fold, DatasetDir, GeocellId, Lod1x, TileFileName};

    /// Requirement 65 (§8.6.2.1) and Requirement 66 (§8.6.2.2): geocell
    /// directory names encode the southwest corner; longitude directories
    /// align to the zone width of Table 3-29.
    #[test]
    fn r1x_geocell_dir_roundtrip() {
        let g = GeocellId::from_dirs("S06", "E045").unwrap();
        assert_eq!((g.lat_sw, g.lon_sw), (-6, 45));
        assert_eq!(g.lat_dir_name(), "S06");
        assert_eq!(g.lon_dir_name(), "E045");
        let g = GeocellId::from_dirs("N62", "W162").unwrap();
        assert_eq!((g.lat_sw, g.lon_sw), (62, -162));
        assert_eq!(g.zone_width(), 2);

        let g = GeocellId::from_dirs("S90", "W180").unwrap();
        assert_eq!((g.lat_sw, g.lon_sw), (-90, -180));
        assert_eq!(g.lat_dir_name(), "S90");
        assert_eq!(g.lon_dir_name(), "W180");
    }

    /// Folded grammar names remain parseable, while formatting is canonical.
    #[test]
    fn mig_geocell_dir_accepts_ascii_case_variants() {
        let g = GeocellId::from_dirs("s06", "e045").unwrap();
        assert_eq!((g.lat_sw, g.lon_sw), (-6, 45));
        assert_eq!(g.lat_dir_name(), "S06");
        assert_eq!(g.lon_dir_name(), "E045");

        let g = GeocellId::from_dirs("n62", "w162").unwrap();
        assert_eq!((g.lat_sw, g.lon_sw), (62, -162));
        assert_eq!(g.lat_dir_name(), "N62");
        assert_eq!(g.lon_dir_name(), "W162");
    }

    /// Table 3-29 band edges, both hemispheres.
    #[test]
    fn r1x_geocell_zone_widths_match_table_3_29() {
        let w = |lat: i8| {
            GeocellId {
                lat_sw: lat,
                lon_sw: 0,
            }
            .zone_width()
        };
        assert_eq!(w(0), 1);
        assert_eq!(w(49), 1);
        assert_eq!(w(50), 2);
        assert_eq!(w(69), 2);
        assert_eq!(w(70), 3);
        assert_eq!(w(75), 4);
        assert_eq!(w(80), 6);
        assert_eq!(w(89), 12);
        assert_eq!(w(-50), 1);
        assert_eq!(w(-51), 2);
        assert_eq!(w(-70), 2);
        assert_eq!(w(-71), 3);
        assert_eq!(w(-90), 12);
    }

    /// Rejections: bad prefix, out of range, zone-misaligned longitude.
    #[test]
    fn r1x_geocell_dir_rejects() {
        assert!(GeocellId::from_dirs("X06", "E045").is_err());
        assert!(GeocellId::from_dirs("N90", "E000").is_err());
        assert!(GeocellId::from_dirs("N62", "W161").is_err());
        assert!(GeocellId::from_dirs("N62", "E180").is_err());
    }

    /// Numeric fields are fixed-width ASCII digits without signs or Unicode.
    #[test]
    fn mig_geocell_dir_rejects_non_ascii_or_signed_digits() {
        for lat in ["N6", "N006", "N+6", "N-6", "N٠٦", "N０６", "N0é"] {
            assert!(GeocellId::from_dirs(lat, "E045").is_err(), "{lat:?}");
        }
        for lon in ["E45", "E0045", "E+45", "E-45", "E٠٤٥", "E０４５", "E04é"] {
            assert!(GeocellId::from_dirs("N06", lon).is_err(), "{lon:?}");
        }
    }

    /// Negative-direction zero spellings are outside the supported canonical grammar.
    #[test]
    fn mig_geocell_dir_rejects_noncanonical_negative_zero() {
        assert!(GeocellId::from_dirs("S00", "E000").is_err());
        assert!(GeocellId::from_dirs("N00", "W000").is_err());
        assert!(GeocellId::from_dirs("s00", "e000").is_err());
        assert!(GeocellId::from_dirs("n00", "w000").is_err());
    }

    /// Public fields can be constructed directly, so validation catches invalid values.
    #[test]
    fn mig_geocell_validate_rejects_invalid_public_fields() {
        assert!(GeocellId {
            lat_sw: 90,
            lon_sw: 0,
        }
        .validate()
        .is_err());
        assert!(GeocellId {
            lat_sw: 62,
            lon_sw: -161,
        }
        .validate()
        .is_err());
        assert!(GeocellId {
            lat_sw: 0,
            lon_sw: 180,
        }
        .validate()
        .is_err());
    }

    /// Public-field arithmetic stays defined across the integer field domains.
    #[test]
    fn mig_geocell_public_field_arithmetic_is_safe() {
        let extreme = GeocellId {
            lat_sw: i8::MIN,
            lon_sw: i16::MIN,
        };
        assert!(extreme.validate().is_err());
        assert_eq!(extreme.zone_width(), 12);
        assert_eq!(extreme.lat_dir_name(), "S128");
        assert_eq!(extreme.lon_dir_name(), "W32768");
    }

    /// The fold is ASCII lower-casing and nothing else.
    #[test]
    fn mig_fold_is_ascii_lowercase_only() {
        assert_eq!(
            fold("N32W118_D201_S002_T003_LC05_U0_R0.shp"),
            "n32w118_d201_s002_t003_lc05_u0_r0.shp"
        );
        assert_eq!(fold("already_lower"), "already_lower");
        assert_eq!(fold("CAFÉ"), "cafÉ");
    }

    /// LOD designation rule (15-113r6 line "C in lieu of the minus sign"):
    /// L00..L23 positive, LC01..LC10 negative; LC is the directory for all
    /// negative LODs (§8.6.2.4).
    #[test]
    fn r1x_lod_tokens_roundtrip() {
        assert_eq!(Lod1x::parse("L00").unwrap().value(), 0);
        assert_eq!(Lod1x::parse("L23").unwrap().value(), 23);
        assert_eq!(Lod1x::parse("LC05").unwrap().value(), -5);
        assert_eq!(Lod1x::parse("LC10").unwrap().value(), -10);
        assert_eq!(Lod1x::parse("L02").unwrap().dir_name(), "L02");
        assert_eq!(Lod1x::parse("LC05").unwrap().dir_name(), "LC");
        assert_eq!(Lod1x::parse("LC05").unwrap().token(), "LC05");
        assert_eq!(Lod1x::parse("lc05").unwrap().token(), "LC05");
        assert!(Lod1x::parse("L24").is_err());
        assert!(Lod1x::parse("LC00").is_err());
        assert!(Lod1x::parse("LC11").is_err());
        assert!(Lod1x::parse("L2").is_err());
    }

    /// LOD digits are exact-width ASCII tokens without signs or Unicode.
    #[test]
    fn mig_lod_rejects_signed_or_non_ascii_digits() {
        for token in ["L+2", "L-2", "L٠٢", "L０２", "LC+5", "LC-5", "LC٠٥"] {
            assert!(Lod1x::parse(token).is_err(), "{token:?}");
        }
    }

    /// Public construction may bypass the parser, so formatting remains
    /// defined at both integer extremes without narrow signed negation.
    #[test]
    fn mig_lod_public_field_extremes_format_without_overflow() {
        let minimum = Lod1x(i8::MIN);
        assert_eq!(minimum.value(), i8::MIN);
        assert_eq!(minimum.dir_name(), "LC");
        assert_eq!(minimum.token(), "LC128");

        let maximum = Lod1x(i8::MAX);
        assert_eq!(maximum.value(), i8::MAX);
        assert_eq!(maximum.dir_name(), "L127");
        assert_eq!(maximum.token(), "L127");
    }

    /// Dataset directory `nnn_Name` (§8.6.2.3).
    #[test]
    fn r1x_dataset_dir_roundtrip() {
        let d = DatasetDir::parse("201_RoadNetwork").unwrap();
        assert_eq!((d.code, d.name.as_str()), (201, "RoadNetwork"));
        assert_eq!(d.dir_name(), "201_RoadNetwork");
        let folded = DatasetDir::parse("201_roadnetwork").unwrap();
        assert_eq!(folded.dir_name(), "201_roadnetwork");
        assert!(DatasetDir::parse("20_RoadNetwork").is_err());
        assert!(DatasetDir::parse("201-RoadNetwork").is_err());
        assert!(DatasetDir::parse("201_").is_err());
    }

    /// Dataset codes are exactly three unsigned ASCII digits.
    #[test]
    fn mig_dataset_dir_rejects_signed_or_non_ascii_codes() {
        for dir in [
            "+01_RoadNetwork",
            "-01_RoadNetwork",
            "٢٠١_RoadNetwork",
            "２０１_RoadNetwork",
        ] {
            assert!(DatasetDir::parse(dir).is_err(), "{dir:?}");
        }
    }

    /// Requirement 67 (§8.6.3.1): LatLon_Dnnn_Snnn_Tnnn_LOD_Un_Rn.<ext>,
    /// spec's own examples verbatim.
    #[test]
    fn r1x_tile_file_name_spec_examples() {
        let t = TileFileName::parse("S06E045_D001_S001_T001_L02_U3_R0.tif").unwrap();
        assert_eq!((t.geocell.lat_sw, t.geocell.lon_sw), (-6, 45));
        assert_eq!((t.dataset, t.cs1, t.cs2), (1, 1, 1));
        assert_eq!((t.lod.value(), t.uref, t.rref), (2, 3, 0));
        assert_eq!(t.extension, "tif");
        assert_eq!(t.canonical(), "S06E045_D001_S001_T001_L02_U3_R0.tif");

        let t = TileFileName::parse("N62W162_D100_S001_T001_L07_U38_R102.shp").unwrap();
        assert_eq!((t.uref, t.rref), (38, 102));

        let t = TileFileName::parse("N32W118_D201_S002_T003_LC05_U0_R0.dbf").unwrap();
        assert_eq!(t.lod.value(), -5);
    }

    /// Folded grammar spelling remains parseable and canonical formatting
    /// restores the spec case rather than retaining arbitrary input case.
    #[test]
    fn mig_fold_inverts_via_grammar() {
        let original = "N32W118_D201_S002_T003_LC05_U0_R0.shp";
        let folded = fold(original);
        let parsed = TileFileName::parse(&folded).unwrap();
        assert_eq!(parsed.canonical(), original);

        let parsed = TileFileName::parse("n32w118_d201_s002_t003_lc05_u0_r0.SHP").unwrap();
        assert_eq!(parsed.canonical(), original);
    }

    /// Malformed seven-byte geocell fields containing a split UTF-8 code
    /// point return an error instead of panicking at a byte boundary.
    #[test]
    fn mig_tile_file_name_rejects_non_ascii_geocell_without_panicking() {
        let malformed = "N0éE04_D001_S001_T001_L02_U3_R0.tif";
        assert!(TileFileName::parse(malformed).is_err());
    }

    /// Requirement 67 fields reject signs, Unicode digits, overflow, missing
    /// values, and fields after R rather than accepting partial parses.
    #[test]
    fn mig_tile_file_name_rejects_malformed_numeric_or_trailing_fields() {
        for file in [
            "N32W118_D+01_S002_T003_L02_U0_R0.shp",
            "N32W118_D201_S-02_T003_L02_U0_R0.shp",
            "N32W118_D201_S002_T٠٠٣_L02_U0_R0.shp",
            "N32W118_D201_S002_T003_L02_U-1_R0.shp",
            "N32W118_D201_S002_T003_L02_U0_R+1.shp",
            "N32W118_D201_S002_T003_L02_U4294967296_R0.shp",
            "N32W118_D201_S002_T003_L02_U_R0.shp",
            "N32W118_D201_S002_T003_L02_U0_R0_X1.shp",
        ] {
            assert!(TileFileName::parse(file).is_err(), "{file:?}");
        }
    }

    /// An extension is required and must contain only ASCII token characters.
    #[test]
    fn mig_tile_file_name_rejects_missing_empty_or_non_ascii_extension() {
        for file in [
            "N32W118_D201_S002_T003_L02_U0_R0",
            "N32W118_D201_S002_T003_L02_U0_R0.",
            "N32W118_D201_S002_T003_L02_U0_R0.é",
            "N32W118_D201_S002_T003_L02_U0_R0.ti-f",
            "N32W118_D201_S002_T003_L02_U0_R0.tif_",
        ] {
            assert!(TileFileName::parse(file).is_err(), "{file:?}");
        }
    }
}
