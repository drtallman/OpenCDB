//! Structural inspection of the GeoPackage 1.2.1 metadata subset.
use super::io::sql_error;
use super::{MetadataError, RecordKind, malformed, unsupported};
use rusqlite::Connection;
use std::collections::BTreeSet;

const TABLES: [&str; 5] = [
    "gpkg_spatial_ref_sys",
    "gpkg_contents",
    "gpkg_extensions",
    "gpkg_metadata",
    "gpkg_metadata_reference",
];
// (name, declared type, not-null, primary-key position), from OGC 12-128r15.
type Column = (&'static str, &'static str, bool, i64);
const SRS: &[Column] = &[
    ("srs_name", "TEXT", true, 0),
    ("srs_id", "INTEGER", true, 1),
    ("organization", "TEXT", true, 0),
    ("organization_coordsys_id", "INTEGER", true, 0),
    ("definition", "TEXT", true, 0),
    ("description", "TEXT", false, 0),
];
const CONTENTS: &[Column] = &[
    ("table_name", "TEXT", true, 1),
    ("data_type", "TEXT", true, 0),
    ("identifier", "TEXT", false, 0),
    ("description", "TEXT", false, 0),
    ("last_change", "DATETIME", true, 0),
    ("min_x", "DOUBLE", false, 0),
    ("min_y", "DOUBLE", false, 0),
    ("max_x", "DOUBLE", false, 0),
    ("max_y", "DOUBLE", false, 0),
    ("srs_id", "INTEGER", false, 0),
];
const EXTENSIONS: &[Column] = &[
    ("table_name", "TEXT", false, 0),
    ("column_name", "TEXT", false, 0),
    ("extension_name", "TEXT", true, 0),
    ("definition", "TEXT", true, 0),
    ("scope", "TEXT", true, 0),
];
const METADATA: &[Column] = &[
    ("id", "INTEGER", true, 1),
    ("md_scope", "TEXT", true, 0),
    ("md_standard_uri", "TEXT", true, 0),
    ("mime_type", "TEXT", true, 0),
    ("metadata", "TEXT", true, 0),
];
const REFERENCE: &[Column] = &[
    ("reference_scope", "TEXT", true, 0),
    ("table_name", "TEXT", false, 0),
    ("column_name", "TEXT", false, 0),
    ("row_id_value", "INTEGER", false, 0),
    ("timestamp", "DATETIME", true, 0),
    ("md_file_id", "INTEGER", true, 0),
    ("md_parent_id", "INTEGER", false, 0),
];

pub(super) fn read_document(conn: &Connection, kind: RecordKind) -> Result<String, MetadataError> {
    let app: i64 = conn
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .map_err(sql_error)?;
    if app != 0x4750_4B47 {
        return Err(malformed("incorrect GeoPackage application ID"));
    }
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(sql_error)?;
    if version != 10200 {
        return Err(unsupported(format!(
            "container version {version}; expected 10200"
        )));
    }
    let check: String = conn
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(sql_error)?;
    if check != "ok" {
        return Err(malformed(check));
    }
    inventory(conn)?;
    for (table, columns) in TABLES
        .into_iter()
        .zip([SRS, CONTENTS, EXTENSIONS, METADATA, REFERENCE])
    {
        check_columns(conn, table, columns)?;
    }
    for (table, column, expected) in [
        ("gpkg_contents", "description", "''"),
        (
            "gpkg_contents",
            "last_change",
            "strftime('%Y-%m-%dT%H:%M:%fZ','now')",
        ),
        ("gpkg_metadata", "md_scope", "'dataset'"),
        ("gpkg_metadata", "mime_type", "'text/xml'"),
        ("gpkg_metadata", "metadata", "''"),
        (
            "gpkg_metadata_reference",
            "timestamp",
            "strftime('%Y-%m-%dT%H:%M:%fZ','now')",
        ),
    ] {
        let actual: Option<String> = conn
            .query_row(
                "SELECT dflt_value FROM pragma_table_info(?1) WHERE name=?2 COLLATE NOCASE",
                [table, column],
                |r| r.get(0),
            )
            .map_err(sql_error)?;
        if !actual.is_some_and(|value| equivalent_default(&value, expected)) {
            return Err(malformed(format!("invalid default for {table}.{column}")));
        }
    }
    check_unique(conn, "gpkg_contents", &["identifier"])?;
    check_unique(
        conn,
        "gpkg_extensions",
        &["table_name", "column_name", "extension_name"],
    )?;
    check_foreign_keys(
        conn,
        "gpkg_contents",
        &[("srs_id", "gpkg_spatial_ref_sys", "srs_id")],
    )?;
    check_foreign_keys(
        conn,
        "gpkg_metadata_reference",
        &[
            ("md_file_id", "gpkg_metadata", "id"),
            ("md_parent_id", "gpkg_metadata", "id"),
        ],
    )?;
    let broken: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .map_err(sql_error)?;
    if broken != 0 {
        return Err(malformed("dangling GeoPackage foreign key"));
    }
    check_srs(conn)?;
    let contents: i64 = conn
        .query_row("SELECT count(*) FROM gpkg_contents", [], |r| r.get(0))
        .map_err(sql_error)?;
    if contents != 0 {
        return Err(unsupported(
            "user data is outside the metadata document binding",
        ));
    }
    check_extensions(conn)?;
    let count: i64 = conn
        .query_row("SELECT count(*) FROM gpkg_metadata", [], |r| r.get(0))
        .map_err(sql_error)?;
    if count > 1 {
        return Err(unsupported("multiple metadata documents"));
    }
    if count == 0 {
        return Err(malformed("missing metadata document"));
    }
    let (id, scope, uri, mime, json): (i64, String, String, String, String) = conn
        .query_row(
            "SELECT id, md_scope, md_standard_uri, mime_type, metadata FROM gpkg_metadata",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .map_err(sql_error)?;
    if uri != kind.schema_uri() {
        return Err(unsupported(format!(
            "unrecognized binding for {kind:?}: {uri}"
        )));
    }
    if scope != "dataset" || mime != "application/json" {
        return Err(malformed(
            "binding requires dataset scope and application/json MIME",
        ));
    }
    check_reference(conn, id)?;
    Ok(json)
}

fn inventory(conn: &Connection) -> Result<(), MetadataError> {
    let mut stmt = conn
        .prepare("SELECT type, name FROM sqlite_schema")
        .map_err(sql_error)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(sql_error)?;
    let mut tables = BTreeSet::new();
    for row in rows {
        let (kind, name) = row.map_err(sql_error)?;
        let name = name.to_ascii_lowercase();
        match kind.as_str() {
            "view" | "trigger" => return Err(unsupported(format!("{kind} {name}"))),
            "table" if name.starts_with("sqlite_") => {}
            "table" if TABLES.contains(&name.as_str()) => {
                tables.insert(name);
            }
            "index" => {}
            _ => return Err(unsupported(format!("schema object {name}"))),
        }
    }
    if tables.len() != TABLES.len() {
        return Err(malformed("missing required GeoPackage table"));
    }
    Ok(())
}

fn check_columns(conn: &Connection, table: &str, expected: &[Column]) -> Result<(), MetadataError> {
    let mut stmt = conn
        .prepare("SELECT name,type,\"notnull\",pk,hidden FROM pragma_table_xinfo(?1)")
        .map_err(sql_error)?;
    let rows = stmt
        .query_map([table], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, bool>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })
        .map_err(sql_error)?;
    let columns: Vec<_> = rows.collect::<Result<_, _>>().map_err(sql_error)?;
    for (name, ty, notnull, pk) in expected {
        if !columns.iter().any(|(n, t, nn, p, h)| {
            n.eq_ignore_ascii_case(name)
                && t.eq_ignore_ascii_case(ty)
                && nn == notnull
                && p == pk
                && *h == 0
        }) {
            return Err(malformed(format!(
                "invalid or missing {table}.{name} column definition"
            )));
        }
    }
    if columns.len() != expected.len() {
        return Err(unsupported(format!("extra columns in {table}")));
    }
    Ok(())
}

fn check_unique(conn: &Connection, table: &str, columns: &[&str]) -> Result<(), MetadataError> {
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_index_list(?1) WHERE \"unique\"=1 AND partial=0")
        .map_err(sql_error)?;
    let indexes = stmt
        .query_map([table], |r| r.get::<_, String>(0))
        .map_err(sql_error)?;
    for index in indexes {
        let index = index.map_err(sql_error)?;
        let mut fields = conn
            .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
            .map_err(sql_error)?;
        let names = fields
            .query_map([index], |r| r.get::<_, Option<String>>(0))
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?;
        let expected: BTreeSet<_> = columns
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect();
        let actual: Option<BTreeSet<_>> = names
            .iter()
            .map(|name| name.as_ref().map(|name| name.to_ascii_lowercase()))
            .collect();
        if names.len() == columns.len() && actual.as_ref() == Some(&expected) {
            return Ok(());
        }
    }
    Err(malformed(format!("missing unique key in {table}")))
}

fn check_foreign_keys(
    conn: &Connection,
    table: &str,
    expected: &[(&str, &str, &str)],
) -> Result<(), MetadataError> {
    let mut stmt = conn
        .prepare("SELECT \"from\",\"table\",\"to\",seq FROM pragma_foreign_key_list(?1)")
        .map_err(sql_error)?;
    let rows = stmt
        .query_map([table], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    for (from, target, to) in expected {
        if !rows.iter().any(|(f, t, c, s)| {
            f.eq_ignore_ascii_case(from)
                && t.eq_ignore_ascii_case(target)
                && c.eq_ignore_ascii_case(to)
                && *s == 0
        }) {
            return Err(malformed(format!("missing foreign key {table}.{from}")));
        }
    }
    if rows.len() != expected.len() {
        return Err(unsupported(format!("extra relationships in {table}")));
    }
    Ok(())
}

fn check_srs(conn: &Connection) -> Result<(), MetadataError> {
    for (id, org) in [(-1, "NONE"), (0, "NONE"), (4326, "EPSG")] {
        let mut stmt = conn.prepare("SELECT srs_name, organization, organization_coordsys_id, definition FROM gpkg_spatial_ref_sys WHERE srs_id=?1").map_err(sql_error)?;
        let mut rows = stmt.query([id]).map_err(sql_error)?;
        let row = rows
            .next()
            .map_err(sql_error)?
            .ok_or_else(|| malformed(format!("missing reserved SRS {id}")))?;
        let name: String = row.get(0).map_err(sql_error)?;
        let organization: String = row.get(1).map_err(sql_error)?;
        let org_id: i64 = row.get(2).map_err(sql_error)?;
        let definition: String = row.get(3).map_err(sql_error)?;
        if name.trim().is_empty()
            || !organization.eq_ignore_ascii_case(org)
            || org_id != id
            || definition.trim().is_empty()
            || (id != 4326 && definition != "undefined")
        {
            return Err(malformed(format!("invalid reserved SRS {id}")));
        }
        if id == 4326 {
            check_wgs84(&definition)?;
        }
    }
    Ok(())
}

fn check_extensions(conn: &Connection) -> Result<(), MetadataError> {
    let mut stmt = conn
        .prepare(
            "SELECT table_name,column_name,extension_name,definition,scope FROM gpkg_extensions",
        )
        .map_err(sql_error)?;
    let mut rows = stmt.query([]).map_err(sql_error)?;
    let mut found = BTreeSet::new();
    while let Some(row) = rows.next().map_err(sql_error)? {
        let table: Option<String> = row.get(0).map_err(sql_error)?;
        let column: Option<String> = row.get(1).map_err(sql_error)?;
        let name: String = row.get(2).map_err(sql_error)?;
        let definition: String = row.get(3).map_err(sql_error)?;
        let scope: String = row.get(4).map_err(sql_error)?;
        if name != "gpkg_metadata" {
            return Err(unsupported(format!("extension {name}")));
        }
        let table = table.unwrap_or_default().to_ascii_lowercase();
        if !["gpkg_metadata", "gpkg_metadata_reference"].contains(&table.as_str())
            || column.is_some()
            || scope != "read-write"
            || !matches!(
                definition.as_str(),
                "http://www.geopackage.org/spec121/#extension_metadata"
                    | "https://www.geopackage.org/spec121/#extension_metadata"
            )
        {
            return Err(malformed("invalid metadata extension registration"));
        }
        if !found.insert(table) {
            return Err(malformed("duplicate metadata extension registration"));
        }
    }
    if found.len() != 2 {
        return Err(malformed("missing metadata extension registration"));
    }
    Ok(())
}

fn check_reference(conn: &Connection, id: i64) -> Result<(), MetadataError> {
    let count: i64 = conn
        .query_row("SELECT count(*) FROM gpkg_metadata_reference", [], |r| {
            r.get(0)
        })
        .map_err(sql_error)?;
    if count > 1 {
        return Err(unsupported("multiple metadata references"));
    }
    if count == 0 {
        return Err(malformed("missing package reference"));
    }
    let (valid, timestamp): (bool, String) = conn.query_row(
        "SELECT reference_scope='geopackage' AND table_name IS NULL AND column_name IS NULL AND row_id_value IS NULL AND md_file_id=?1 AND md_parent_id IS NULL, timestamp FROM gpkg_metadata_reference",
        [id], |r| Ok((r.get(0)?,r.get(1)?))).map_err(sql_error)?;
    let canonical = chrono::DateTime::parse_from_rfc3339(&timestamp)
        .ok()
        .is_some_and(|t| {
            t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true) == timestamp
                && timestamp.len() == 24
                && timestamp.ends_with('Z')
        });
    if !valid || !canonical {
        return Err(malformed(
            "invalid package reference or UTC millisecond timestamp",
        ));
    }
    Ok(())
}

// Compare only the tiny literal/default-expression grammar defined by the standard.
// Never evaluate database-authored SQL to inspect a default.
fn equivalent_default(actual: &str, expected: &str) -> bool {
    let mut value = actual.trim();
    while value.starts_with('(') && value.ends_with(')') {
        value = value[1..value.len() - 1].trim();
    }
    if expected.starts_with("strftime") {
        let mut normalized = String::new();
        let mut quoted = false;
        for ch in value.chars() {
            if ch == '\'' {
                quoted = !quoted;
            }
            if quoted {
                normalized.push(ch);
            } else if !ch.is_ascii_whitespace() {
                normalized.push(ch.to_ascii_lowercase());
            }
        }
        normalized == expected
    } else {
        value == expected
    }
}

// GeoPackage 1.2.1 uses WKT-1 here. Only reuse the keyword-tree tokenizer;
// never run CDB's WKT-2 CRS classification or coordinate-system checks.
fn check_wgs84(definition: &str) -> Result<(), MetadataError> {
    use crate::crs::wkt2::{self, WktValue};
    let tree = wkt2::parse(definition).map_err(|error| malformed(error.to_string()))?;
    if tree.keyword != "GEOGCS" {
        return Err(malformed(
            "reserved SRS 4326 requires a WKT-1 geographic CRS",
        ));
    }
    let datum = tree
        .find("DATUM")
        .ok_or_else(|| malformed("WGS 84 datum missing"))?;
    let spheroid = datum
        .find("SPHEROID")
        .ok_or_else(|| malformed("WGS 84 spheroid missing"))?;
    let dimensions: Vec<_> = spheroid
        .arguments
        .iter()
        .filter_map(|v| match v {
            WktValue::Number(n) => Some(*n),
            _ => None,
        })
        .collect();
    if dimensions.len() != 2
        || (dimensions[0] - 6378137.0).abs() > 1e-8
        || (dimensions[1] - 298.257223563).abs() > 1e-9
    {
        return Err(malformed("reserved SRS 4326 has an incompatible spheroid"));
    }
    if tree.find("PRIMEM").and_then(|n| n.first_number()) != Some(0.0)
        || !tree
            .find("UNIT")
            .and_then(|n| n.first_number())
            .is_some_and(|n| (n - std::f64::consts::PI / 180.0).abs() < 1e-15)
    {
        return Err(malformed(
            "reserved SRS 4326 requires Greenwich and angular degrees",
        ));
    }
    let datum_name: String = datum
        .name()
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_uppercase)
        .collect();
    if !["WGS84", "WGS1984", "WORLDGEODETICSYSTEM1984"].contains(&datum_name.as_str()) {
        let authority = datum.find("AUTHORITY");
        let is_wgs84 = authority.is_some_and(|node| matches!(node.arguments.as_slice(), [WktValue::Text(org), WktValue::Text(id)] if org.eq_ignore_ascii_case("EPSG") && id == "6326"));
        if !is_wgs84 {
            return Err(unsupported("unrecognized WGS 84 datum name or authority"));
        }
    }
    if datum.find("TOWGS84").is_some_and(|node| {
        node.arguments
            .iter()
            .any(|v| !matches!(v, WktValue::Number(0.0)))
    }) {
        return Err(malformed(
            "reserved SRS 4326 has a nonidentity datum transformation",
        ));
    }
    Ok(())
}
