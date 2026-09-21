//! Maps CDB 1.x per-geocell tile coordinates to and from `Cdb1GlobalGrid`.
//!
//! CDB 1.x counts UREF north from a geocell's south edge and RREF east from
//! its west edge (§8.6.2.5, §8.6.3.1). `Cdb1GlobalGrid` counts rows south
//! from the north pole and uses nominal, uncoalesced global columns.

use opencdb::{Cdb1GlobalGrid, Cdb1Lod, Cdb1TileAddress};

use crate::grammar::{GeocellId, Lod1x, TileFileName};

/// A dataset-independent CDB 1.x tile address within one geocell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cdb1PerGeocellAddress {
    /// The geocell containing the tile.
    pub geocell: GeocellId,
    /// The CDB 1.x level of detail.
    pub lod: Lod1x,
    /// Row within the geocell, counted north from its south edge.
    pub uref: u32,
    /// Column within the geocell, counted east from its west edge.
    pub rref: u32,
}

/// Converts a parsed CDB 1.x tile name to its validated global-grid address.
///
/// Publicly constructible grammar fields are validated before arithmetic.
/// The conversion reconstructs a point strictly inside the source tile and
/// delegates global row, nominal-column, and coalescence conventions to
/// [`Cdb1GlobalGrid::tile_at`]. Dataset selectors and file encoding do not
/// participate in the address.
pub fn global_address(file: &TileFileName) -> Result<Cdb1TileAddress, String> {
    file.geocell.validate()?;
    let lod = Cdb1Lod::new(file.lod.value()).map_err(|error| format!("{error:?}"))?;
    let per_geocell = tiles_per_geocell(file.lod.value());
    if file.uref >= per_geocell || file.rref >= per_geocell {
        return Err(format!(
            "U{}/R{} out of range for {} ({} per geocell)",
            file.uref,
            file.rref,
            file.lod.token(),
            per_geocell
        ));
    }

    let width = f64::from(file.geocell.zone_width());
    let lat =
        f64::from(file.geocell.lat_sw) + (f64::from(file.uref) + 0.5) / f64::from(per_geocell);
    let lon = f64::from(file.geocell.lon_sw)
        + (f64::from(file.rref) + 0.5) * width / f64::from(per_geocell);
    Cdb1GlobalGrid::tile_at(lat, lon, lod).map_err(|error| format!("{error:?}"))
}

/// Converts a validated global-grid address to CDB 1.x per-geocell fields.
///
/// This inverse uses integer address arithmetic. `col` remains the nominal,
/// uncoalesced column index exposed by `opencdb`; the latitude zone width is
/// applied only when recovering the geocell and RREF.
pub fn per_geocell_address(address: Cdb1TileAddress) -> Cdb1PerGeocellAddress {
    let lod_value = address.lod().value();
    let per_geocell = u64::from(tiles_per_geocell(lod_value));

    let geocell_row = address.row() / per_geocell;
    let lat_sw = (89_i64 - geocell_row as i64) as i8;
    let uref = (per_geocell - 1 - address.row() % per_geocell) as u32;

    let latitude_cell = GeocellId { lat_sw, lon_sw: 0 };
    let zone_width = u64::from(latitude_cell.zone_width());
    let nominal_columns_per_geocell = per_geocell * zone_width;
    let geocell_col = address.col() / nominal_columns_per_geocell;
    let lon_sw = (-180_i64 + (geocell_col * zone_width) as i64) as i16;
    let rref = ((address.col() % nominal_columns_per_geocell) / zone_width) as u32;

    Cdb1PerGeocellAddress {
        geocell: GeocellId { lat_sw, lon_sw },
        lod: Lod1x(lod_value),
        uref,
        rref,
    }
}

fn tiles_per_geocell(lod: i8) -> u32 {
    if lod <= 0 {
        1
    } else {
        1_u32 << lod
    }
}

#[cfg(test)]
mod tests {
    use opencdb::{Cdb1GlobalGrid, Cdb1Lod, Cdb1TileAddress};

    use super::{global_address, per_geocell_address, Cdb1PerGeocellAddress};
    use crate::grammar::{GeocellId, Lod1x, TileFileName};

    fn file(lat_sw: i8, lon_sw: i16, lod: i8, uref: u32, rref: u32) -> TileFileName {
        TileFileName {
            geocell: GeocellId { lat_sw, lon_sw },
            dataset: 1,
            cs1: 1,
            cs2: 1,
            lod: Lod1x(lod),
            uref,
            rref,
            extension: "tif".to_string(),
        }
    }

    fn parts(address: Cdb1TileAddress) -> (i8, u64, u64) {
        (address.lod().value(), address.row(), address.col())
    }

    fn extent(address: Cdb1TileAddress) -> (f64, f64, f64, f64) {
        let bbox = Cdb1GlobalGrid::tile_extent(address);
        (bbox.west, bbox.south, bbox.east, bbox.north)
    }

    /// Independent hand-calculated global addresses and footprints catch a
    /// flipped UREF axis, packed instead of nominal columns, and wrong zone width.
    #[test]
    fn mig_bridge_has_independent_expected_addresses_and_extents() {
        let cases = [
            (
                "N32W118_D201_S002_T003_LC05_U0_R0.shp",
                (-5, 57, 62),
                (-118.0, 32.0, -117.0, 33.0),
            ),
            (
                "N32W118_D001_S001_T001_L01_U1_R0.tif",
                (1, 114, 124),
                (-118.0, 32.5, -117.5, 33.0),
            ),
            (
                "N62W162_D100_S001_T001_L07_U38_R102.shp",
                (7, 3545, 2508),
                (-160.40625, 62.296875, -160.390625, 62.3046875),
            ),
            (
                "S06E045_D001_S001_T001_L02_U3_R0.tif",
                (2, 380, 900),
                (45.0, -5.25, 45.25, -5.0),
            ),
        ];

        for (name, expected_address, expected_extent) in cases {
            let parsed = TileFileName::parse(name).unwrap();
            let address = global_address(&parsed).unwrap();
            assert_eq!(parts(address), expected_address, "case {name}");
            assert_eq!(extent(address), expected_extent, "case {name}");
            assert_eq!(
                per_geocell_address(address),
                Cdb1PerGeocellAddress {
                    geocell: parsed.geocell,
                    lod: parsed.lod,
                    uref: parsed.uref,
                    rref: parsed.rref,
                },
                "case {name}"
            );
        }
    }

    /// The bridge also agrees with the grid's point lookup for independently
    /// selected interior coordinates in ordinary, negative, wide-zone, and
    /// southern tiles.
    #[test]
    fn mig_bridge_agrees_with_cdb1globalgrid() {
        let cases = [
            ("N32W118_D001_S001_T001_L01_U1_R0.tif", 32.75, -117.75),
            ("N32W118_D201_S002_T003_LC05_U0_R0.shp", 32.5, -117.5),
            (
                "N62W162_D100_S001_T001_L07_U38_R102.shp",
                62.30078125,
                -160.3984375,
            ),
            ("S06E045_D001_S001_T001_L02_U3_R0.tif", -5.125, 45.125),
        ];

        for (name, lat, lon) in cases {
            let parsed = TileFileName::parse(name).unwrap();
            let bridged = global_address(&parsed).unwrap();
            let lod = Cdb1Lod::new(parsed.lod.value()).unwrap();
            let oracle = Cdb1GlobalGrid::tile_at(lat, lon, lod).unwrap();
            assert_eq!(bridged, oracle, "case {name}");
        }
    }

    /// Every latitude-zone transition is checked on each side of the equator.
    /// Eastmost geocells simultaneously prove nominal-column addresses and
    /// exact closure at the antimeridian.
    #[test]
    fn mig_bridge_roundtrips_all_zone_boundaries_and_southern_edges() {
        // (south latitude, west longitude, expected row, expected nominal col)
        let cases = [
            (49, 179, 40, 359),
            (50, 178, 39, 358),
            (70, 177, 19, 357),
            (75, 176, 14, 356),
            (80, 174, 9, 354),
            (89, 168, 0, 348),
            (-50, 179, 139, 359),
            (-51, 178, 140, 358),
            (-71, 177, 160, 357),
            (-76, 176, 165, 356),
            (-81, 174, 170, 354),
            (-90, 168, 179, 348),
        ];

        for (lat_sw, lon_sw, row, col) in cases {
            let parsed = file(lat_sw, lon_sw, 0, 0, 0);
            let address = global_address(&parsed).unwrap();
            assert_eq!(parts(address), (0, row, col));
            assert_eq!(
                extent(address),
                (
                    f64::from(lon_sw),
                    f64::from(lat_sw),
                    180.0,
                    f64::from(lat_sw) + 1.0,
                )
            );
            assert_eq!(
                per_geocell_address(address),
                Cdb1PerGeocellAddress {
                    geocell: parsed.geocell,
                    lod: parsed.lod,
                    uref: 0,
                    rref: 0,
                }
            );
        }
    }

    /// LOD −10 and 23 exercise both shift extremes; the west and east edge
    /// cases prove antimeridian handling without floating-point tolerance.
    #[test]
    fn mig_bridge_handles_lod_extremes_and_antimeridian() {
        let coarse = file(-90, -180, -10, 0, 0);
        let coarse_address = global_address(&coarse).unwrap();
        assert_eq!(parts(coarse_address), (-10, 179, 0));
        assert_eq!(extent(coarse_address), (-180.0, -90.0, -168.0, -89.0));
        assert_eq!(
            per_geocell_address(coarse_address),
            Cdb1PerGeocellAddress {
                geocell: coarse.geocell,
                lod: coarse.lod,
                uref: 0,
                rref: 0,
            }
        );

        let fine = file(0, 179, 23, 8_388_607, 8_388_607);
        let fine_address = global_address(&fine).unwrap();
        assert_eq!(parts(fine_address), (23, 746_586_112, 3_019_898_879));
        assert_eq!(
            extent(fine_address),
            (179.9999998807907, 0.9999998807907104, 180.0, 1.0,)
        );
        assert_eq!(
            per_geocell_address(fine_address),
            Cdb1PerGeocellAddress {
                geocell: fine.geocell,
                lod: fine.lod,
                uref: fine.uref,
                rref: fine.rref,
            }
        );
    }

    /// All publicly constructible address-bearing fields are revalidated at
    /// the bridge boundary before shifts or coordinate arithmetic.
    #[test]
    fn mig_bridge_rejects_invalid_public_address_fields() {
        let invalid = [
            file(90, 0, 0, 0, 0),
            file(0, 180, 0, 0, 0),
            file(62, -161, 0, 0, 0),
            file(0, 0, -11, 0, 0),
            file(0, 0, 24, 0, 0),
            file(0, 0, 0, 1, 0),
            file(0, 0, 0, 0, 1),
            file(0, 0, 23, 8_388_608, 0),
            file(0, 0, 23, 0, 8_388_608),
        ];

        for invalid_file in invalid {
            assert!(
                global_address(&invalid_file).is_err(),
                "accepted {invalid_file:?}"
            );
        }
    }

    /// Dataset selectors and encoding do not participate in either address
    /// direction; the inverse type cannot manufacture them.
    #[test]
    fn mig_bridge_is_independent_of_dataset_selectors() {
        let mut first = file(32, -118, 1, 1, 0);
        let mut second = first.clone();
        first.dataset = 1;
        first.cs1 = 2;
        first.cs2 = 3;
        first.extension = "tif".to_string();
        second.dataset = 999;
        second.cs1 = 998;
        second.cs2 = 997;
        second.extension = "zip".to_string();

        let first_address = global_address(&first).unwrap();
        let second_address = global_address(&second).unwrap();
        assert_eq!(first_address, second_address);
        assert_eq!(
            per_geocell_address(first_address),
            Cdb1PerGeocellAddress {
                geocell: first.geocell,
                lod: first.lod,
                uref: first.uref,
                rref: first.rref,
            }
        );
    }
}
