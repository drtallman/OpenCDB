//! Synthetic CDB 1.x source used by reader and migration integration tests.
use std::fs;
use std::path::Path;

pub fn build_1x_tree(root: &Path) {
    let mk = |rel: &str, bytes: &[u8]| {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    };
    mk("Metadata/Version.xml", br#"<Version><Specification version="1.2" authority="OGC"/><Metadata standard="DCAT"/></Version>"#);
    mk(
        "Tiles/N32/W118/001_Elevation/L01/U1/N32W118_D001_S001_T001_L01_U1_R0.tif",
        b"ELEV-BYTES",
    );
    mk(
        "Tiles/N32/W118/201_RoadNetwork/LC/U0/N32W118_D201_S002_T003_LC05_U0_R0.shp",
        b"SHP-BYTES",
    );
    mk(
        "GTModel/500_GTModelGeometry/A_Culture/D500_S001_T001_AL015_000_bridge.flt",
        b"FLT-BYTES",
    );
    mk("stray notes.txt", b"junk");
}
