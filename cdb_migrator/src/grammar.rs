//! The CDB 1.x name grammar of OGC 15-113r6 §8.6.2–8.6.3: geocell
//! directories, dataset directories, LOD and UREF directories, and the
//! tiled-dataset file naming convention. Parsing accepts ASCII case variants;
//! formatting reconstructs the uppercase canonical spelling. Callers that
//! need original-byte provenance preserve the raw source path separately.

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

#[cfg(test)]
mod tests {
    use super::{fold, GeocellId};

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
}
