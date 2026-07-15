//! Media Types requirements module (spec §7.8) — optional.
//!
//! Enumerates the media types for CDB metadata and datasets from the §7.8.1
//! table. The spec says the table "can be extended as required", so unknown
//! strings are preserved via [`MediaType::Other`] rather than rejected.

use std::convert::Infallible;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A media type from the spec table (§7.8.1), or an extension value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MediaType {
    /// `model/flt` — OpenFlight model dataset.
    OpenFlight,
    /// `application/geo+json` — vector dataset as GeoJSON.
    GeoJson,
    /// `application/geopackage+sqlite3` — GeoPackage dataset.
    GeoPackage,
    /// `image/tiff; application=geotiff` — GeoTIFF with georeferencing.
    GeoTiff,
    /// `application/gltf-buffer` — glTF buffer dataset.
    GltfBuffer,
    /// `model/gltf-binary` — glTF binary model dataset.
    GltfBinary,
    /// `model/gltf+json` — glTF JSON model dataset.
    GltfJson,
    /// `application/gml+xml` — GML dataset.
    Gml,
    /// `image/jp2` — JPEG 2000 dataset.
    Jpeg2000,
    /// `application/json` — JSON, usually metadata.
    Json,
    /// `image/png` — PNG image (coverage).
    Png,
    /// `application/vnd.shp` — Shapefile vector dataset.
    Shapefile,
    /// `image/tiff` — TIFF image.
    Tiff,
    /// `application/xml` — XML, usually metadata.
    Xml,
    /// An extension media type not in the spec table.
    Other(String),
}

impl MediaType {
    /// The fourteen entries of the spec table (§7.8.1).
    pub const SPEC_TABLE: [MediaType; 14] = [
        MediaType::OpenFlight,
        MediaType::GeoJson,
        MediaType::GeoPackage,
        MediaType::GeoTiff,
        MediaType::GltfBuffer,
        MediaType::GltfBinary,
        MediaType::GltfJson,
        MediaType::Gml,
        MediaType::Jpeg2000,
        MediaType::Json,
        MediaType::Png,
        MediaType::Shapefile,
        MediaType::Tiff,
        MediaType::Xml,
    ];

    pub fn as_str(&self) -> &str {
        match self {
            MediaType::OpenFlight => "model/flt",
            MediaType::GeoJson => "application/geo+json",
            MediaType::GeoPackage => "application/geopackage+sqlite3",
            MediaType::GeoTiff => "image/tiff; application=geotiff",
            MediaType::GltfBuffer => "application/gltf-buffer",
            MediaType::GltfBinary => "model/gltf-binary",
            MediaType::GltfJson => "model/gltf+json",
            MediaType::Gml => "application/gml+xml",
            MediaType::Jpeg2000 => "image/jp2",
            MediaType::Json => "application/json",
            MediaType::Png => "image/png",
            MediaType::Shapefile => "application/vnd.shp",
            MediaType::Tiff => "image/tiff",
            MediaType::Xml => "application/xml",
            MediaType::Other(value) => value,
        }
    }

    /// Parses a media type string; never fails — non-table values become
    /// [`MediaType::Other`] (the table is extensible).
    pub fn parse(value: &str) -> MediaType {
        MediaType::SPEC_TABLE
            .into_iter()
            .find(|known| known.as_str() == value)
            .unwrap_or_else(|| MediaType::Other(value.to_owned()))
    }

    /// The media type conventionally used for a file extension from the
    /// Requirement Name7 table (plus `png`, which appears in the media type
    /// table but not the extension table).
    pub fn for_extension(extension: &str) -> Option<MediaType> {
        Some(match extension.to_ascii_lowercase().as_str() {
            "flt" => MediaType::OpenFlight,
            "gpkg" => MediaType::GeoPackage,
            "jp2" => MediaType::Jpeg2000,
            "json" => MediaType::Json,
            "png" => MediaType::Png,
            "shp" | "shx" => MediaType::Shapefile,
            "tif" => MediaType::Tiff,
            "xml" | "xsd" => MediaType::Xml,
            "glb" => MediaType::GltfBinary,
            "gltf" => MediaType::GltfJson,
            _ => return None,
        })
    }
}

impl fmt::Display for MediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MediaType {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(MediaType::parse(s))
    }
}

impl Serialize for MediaType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MediaType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Ok(MediaType::parse(&value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §7.8.1 — every table entry round-trips through its string form.
    #[test]
    fn media_type_spec_table_roundtrips() {
        for media_type in MediaType::SPEC_TABLE {
            let text = media_type.as_str().to_owned();
            assert_eq!(MediaType::parse(&text), media_type, "{text}");
        }
    }

    /// §7.8.1 — the exact strings of the spec table.
    #[test]
    fn media_type_uses_spec_strings() {
        assert_eq!(MediaType::OpenFlight.as_str(), "model/flt");
        assert_eq!(MediaType::GeoJson.as_str(), "application/geo+json");
        assert_eq!(
            MediaType::GeoPackage.as_str(),
            "application/geopackage+sqlite3"
        );
        assert_eq!(MediaType::Shapefile.as_str(), "application/vnd.shp");
    }

    /// GeoTIFF is the parameterized form, distinct from plain TIFF.
    #[test]
    fn media_type_geotiff_is_parameterized_tiff() {
        assert_eq!(
            MediaType::GeoTiff.as_str(),
            "image/tiff; application=geotiff"
        );
        assert_eq!(MediaType::Tiff.as_str(), "image/tiff");
        assert_ne!(MediaType::GeoTiff, MediaType::Tiff);
        assert_eq!(MediaType::parse("image/tiff"), MediaType::Tiff);
        assert_eq!(
            MediaType::parse("image/tiff; application=geotiff"),
            MediaType::GeoTiff
        );
    }

    /// The table is extensible: unknown values are preserved, not rejected.
    #[test]
    fn media_type_extension_values_roundtrip() {
        let other = MediaType::parse("application/x-custom");
        assert_eq!(other, MediaType::Other("application/x-custom".into()));
        assert_eq!(other.as_str(), "application/x-custom");
    }

    #[test]
    fn media_type_serde_roundtrip_as_string() {
        let json = serde_json::to_string(&MediaType::GeoTiff).unwrap();
        assert_eq!(json, r#""image/tiff; application=geotiff""#);
        let back: MediaType = serde_json::from_str(&json).unwrap();
        assert_eq!(back, MediaType::GeoTiff);
    }

    /// Extension ↔ media type mapping, consistent with the Name7 table.
    #[test]
    fn media_type_for_extension_consistent_with_name7_table() {
        let cases = [
            ("flt", MediaType::OpenFlight),
            ("gpkg", MediaType::GeoPackage),
            ("jp2", MediaType::Jpeg2000),
            ("json", MediaType::Json),
            ("shp", MediaType::Shapefile),
            ("tif", MediaType::Tiff),
            ("xml", MediaType::Xml),
            ("xsd", MediaType::Xml),
            ("glb", MediaType::GltfBinary),
            ("gltf", MediaType::GltfJson),
        ];
        for (extension, expected) in cases {
            assert_eq!(
                MediaType::for_extension(extension),
                Some(expected),
                "{extension}"
            );
            // Everything mapped here is also in the Name7 extension table.
            assert!(
                crate::naming::known_extension(extension).is_some(),
                "{extension}"
            );
        }
        // png has a media type but is absent from the Name7 extension table.
        assert_eq!(MediaType::for_extension("png"), Some(MediaType::Png));
        assert_eq!(crate::naming::known_extension("png"), None);
        assert_eq!(MediaType::for_extension("unknown"), None);
    }
}
