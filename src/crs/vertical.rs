//! Vertical CRS optional requirements class (spec §7.3.1.7–7.3.1.8).
//!
//! VCRS1: consistent with ISO 19111 (carried by the WKT-2 structure).
//! VCRS2: encoded as WKT for CRS — enforced by parsing.
//! VCRS3: vertical extent units default to meters when unstated.

use super::CrsViolation;
use super::wkt2::{self, CrsKind, UnitKind, UnitSpec, WktNode};

/// A vertical coordinate reference system (`VERTCRS`/`VERTICALCRS`).
#[derive(Debug, Clone, PartialEq)]
pub struct VerticalCrs {
    canonical_wkt: String,
    root: WktNode,
}

impl VerticalCrs {
    /// Parses and validates a WKT-2 vertical CRS (VCRS2).
    pub fn parse(wkt: &str) -> Result<Self, CrsViolation> {
        let root = wkt2::parse(wkt)?;
        if !matches!(wkt2::classify(&root), CrsKind::Vertical) {
            return Err(CrsViolation::NotAVerticalCrs {
                keyword: root.keyword.clone(),
            });
        }
        wkt2::check_coordinate_units(&root)?;
        Ok(Self {
            canonical_wkt: root.to_string(),
            root,
        })
    }

    pub fn name(&self) -> Option<&str> {
        self.root.name()
    }

    pub fn authority(&self) -> Option<(String, String)> {
        wkt2::authority_id(&self.root)
    }

    /// The unit of the vertical axis. Requirement VCRS3: if the units are
    /// not stated, they SHALL be assumed to be meters.
    pub fn unit(&self) -> UnitSpec {
        wkt2::coordinate_units(&self.root)
            .into_iter()
            .find(|unit| matches!(unit.kind, UnitKind::Length | UnitKind::Generic))
            .unwrap_or_else(UnitSpec::metre)
    }

    /// Canonical WKT-2 form.
    pub fn as_wkt(&self) -> &str {
        &self.canonical_wkt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vertical member of the spec's compound example (§7.3.1.9).
    const EGM2008: &str = r#"VERTCRS["EGM2008 height",
  VDATUM["EGM2008 geoid"],
  CS[vertical,1],
    AXIS["gravity-related height (H)",up],
  LENGTHUNIT["metre",1.0],
  ID["EPSG",3855]]"#;

    /// §7.3.1.7 VCRS1/VCRS2 — a WKT-2 vertical CRS parses and classifies.
    #[test]
    fn req_core_crs_vcrs_parses_spec_example() {
        let vcrs = VerticalCrs::parse(EGM2008).unwrap();
        assert_eq!(vcrs.name(), Some("EGM2008 height"));
        assert_eq!(vcrs.authority(), Some(("EPSG".into(), "3855".into())));
        assert_eq!(vcrs.unit().name, "metre");
    }

    /// §7.3.1.8 Requirement VCRS3 — units default to meters when unstated.
    #[test]
    fn req_core_crs_vcrs_units_default_to_meters() {
        let vcrs = VerticalCrs::parse(
            r#"VERTCRS["Depth",VDATUM["Mean Sea Level"],CS[vertical,1],AXIS["depth",down]]"#,
        )
        .unwrap();
        let unit = vcrs.unit();
        assert_eq!(unit.name, "metre");
        assert_eq!(unit.kind, UnitKind::Length);
        assert_eq!(unit.factor, Some(1.0));
    }

    /// Only vertical CRSs are accepted by this class.
    #[test]
    fn req_core_crs_vcrs_rejects_non_vertical() {
        assert!(matches!(
            VerticalCrs::parse(r#"GEOGCRS["WGS 84",DATUM["W",ELLIPSOID["E",6378137,298.25]]]"#),
            Err(CrsViolation::NotAVerticalCrs { .. })
        ));
    }
}
