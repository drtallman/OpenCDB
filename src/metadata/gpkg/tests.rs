use super::*;
fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("record.gpkg");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(include_str!("../../../tests/fixtures/gpkg/independent.sql"))
        .unwrap();
    connection.close().unwrap();
    (tmp, path)
}
/// GeoPackage 1.2.1 Annex F.8: independently constructed metadata extension.
#[test]
fn gpkg_binding_reads_independent_record_with_nondefault_id() {
    let (tmp, path) = fixture();
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        read_document(&path, RecordKind::Resource).unwrap(),
        r#"{"ID":"independent","type":"dataset","title":"Independent","description":"SQL fixture"}"#
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
}

fn directory_snapshot(dir: &Path) -> Vec<(std::ffi::OsString, Vec<u8>)> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (entry.file_name(), std::fs::read(entry.path()).unwrap())
        })
        .collect();
    files.sort();
    files
}

/// GeoPackage 1.2.1 core/Annex F.8 and the OpenCDB single-document binding.
#[test]
fn gpkg_binding_rejects_unsupported_and_malformed_without_mutation() {
    let cases = [
        ("PRAGMA user_version=10400", true),
        (
            "INSERT INTO gpkg_metadata SELECT 43, md_scope, md_standard_uri, mime_type, metadata FROM gpkg_metadata; INSERT INTO gpkg_metadata_reference SELECT reference_scope,table_name,column_name,row_id_value,timestamp,43,md_parent_id FROM gpkg_metadata_reference",
            true,
        ),
        (
            "UPDATE gpkg_metadata SET md_standard_uri='https://github.com/drtallman/OpenCDB/blob/main/docs/GPKG_METADATA.md#global-v1'",
            true,
        ),
        (
            "UPDATE gpkg_metadata SET md_standard_uri='https://example.com/unknown'",
            true,
        ),
        ("CREATE TABLE payload (id INTEGER)", true),
        (
            "INSERT INTO gpkg_extensions VALUES(NULL,NULL,'example_custom','https://example.com/ext','read-write')",
            true,
        ),
        (
            "CREATE VIEW example_view AS SELECT * FROM gpkg_metadata",
            true,
        ),
        (
            "CREATE TRIGGER example_trigger AFTER INSERT ON gpkg_metadata BEGIN SELECT 1; END",
            true,
        ),
        ("DROP TABLE gpkg_metadata_reference", false),
        (
            "DELETE FROM gpkg_extensions WHERE table_name='gpkg_metadata_reference'",
            false,
        ),
        ("UPDATE gpkg_metadata_reference SET md_file_id=99", false),
        ("UPDATE gpkg_metadata SET mime_type='text/xml'", false),
        ("UPDATE gpkg_metadata SET md_scope='invalid'", false),
        (
            "UPDATE gpkg_metadata_reference SET timestamp='2026-09-27T12:00:00+01:00'",
            false,
        ),
        (
            "UPDATE gpkg_metadata_reference SET timestamp='2026-02-30T12:00:00.000Z'",
            false,
        ),
        ("PRAGMA application_id=1234", false),
        ("DELETE FROM gpkg_spatial_ref_sys WHERE srs_id=0", false),
        (
            "UPDATE gpkg_spatial_ref_sys SET organization='WRONG' WHERE srs_id=4326",
            false,
        ),
        ("UPDATE gpkg_extensions SET scope='write-only'", false),
    ];
    for (sql, unsupported) in cases {
        let (tmp, path) = fixture();
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        conn.execute_batch(sql).unwrap();
        conn.close().unwrap();
        let before = directory_snapshot(tmp.path());
        let result = read_document(&path, RecordKind::Resource);
        if unsupported {
            assert!(
                matches!(result, Err(MetadataError::UnsupportedContainer { .. })),
                "{sql}: {result:?}"
            );
        } else {
            assert!(
                matches!(result, Err(MetadataError::Serialization(_))),
                "{sql}: {result:?}"
            );
        }
        assert_eq!(directory_snapshot(tmp.path()), before, "{sql}");
    }
}

/// GeoPackage 1.2.1 table declarations are structural, not literal writer SQL.
#[test]
fn gpkg_binding_accepts_equivalent_case_quoted_tables_and_ordinary_indexes() {
    let (tmp, path) = fixture();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("ALTER TABLE gpkg_metadata RENAME TO temporary_name; ALTER TABLE temporary_name RENAME TO \"GPKG_METADATA\"; CREATE INDEX record_scope ON GPKG_METADATA(md_scope)").unwrap();
    conn.close().unwrap();
    let before = directory_snapshot(tmp.path());
    assert!(read_document(&path, RecordKind::Resource).is_ok());
    assert_eq!(directory_snapshot(tmp.path()), before);
}

/// GeoPackage 1.2.1 Annex F.8 requires the metadata id NOT NULL constraint.
#[test]
fn gpkg_binding_rejects_missing_required_column_constraints() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bad.gpkg");
    let sql = include_str!("../../../tests/fixtures/gpkg/independent.sql").replace(
        "id INTEGER CONSTRAINT m_pk PRIMARY KEY ASC NOT NULL",
        "id INTEGER PRIMARY KEY",
    );
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(&sql).unwrap();
    conn.close().unwrap();
    let before = directory_snapshot(tmp.path());
    assert!(matches!(
        read_document(&path, RecordKind::Resource),
        Err(MetadataError::Serialization(_))
    ));
    assert_eq!(directory_snapshot(tmp.path()), before);
}

/// Read-only binding never creates a missing database or modifies corrupt input.
#[test]
fn gpkg_binding_filesystem_and_corruption_errors_preserve_input() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("absent.gpkg");
    assert!(matches!(
        read_document(&missing, RecordKind::Resource),
        Err(MetadataError::Io(_))
    ));
    assert!(!missing.exists());
    for bytes in [
        b"not a database".as_slice(),
        b"SQLite format 3\0".as_slice(),
    ] {
        std::fs::write(&missing, bytes).unwrap();
        let before = directory_snapshot(tmp.path());
        assert!(matches!(
            read_document(&missing, RecordKind::Resource),
            Err(MetadataError::Serialization(_))
        ));
        assert_eq!(directory_snapshot(tmp.path()), before);
    }
    let (tmp, path) = fixture();
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
    let before = directory_snapshot(tmp.path());
    assert!(matches!(
        read_document(&path, RecordKind::Resource),
        Err(MetadataError::Serialization(_))
    ));
    assert_eq!(directory_snapshot(tmp.path()), before);
}

/// Only finalized standalone containers are supported; SQLite must not recover them.
#[test]
fn gpkg_binding_refuses_pending_companions_and_wal_header() {
    for suffix in ["-wal", "-shm", "-journal"] {
        let (tmp, path) = fixture();
        let mut companion = path.as_os_str().to_owned();
        companion.push(suffix);
        std::fs::write(companion, b"unfinished").unwrap();
        let before = directory_snapshot(tmp.path());
        assert!(matches!(
            read_document(&path, RecordKind::Resource),
            Err(MetadataError::UnsupportedContainer { .. })
        ));
        assert_eq!(directory_snapshot(tmp.path()), before);
    }
    let (tmp, path) = fixture();
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[18] = 2;
    bytes[19] = 2;
    std::fs::write(&path, bytes).unwrap();
    let before = directory_snapshot(tmp.path());
    assert!(matches!(
        read_document(&path, RecordKind::Resource),
        Err(MetadataError::UnsupportedContainer { .. })
    ));
    assert_eq!(directory_snapshot(tmp.path()), before);
}

/// OGC 12-128r15 Table 3 explicitly permits lowercase epsg.
#[test]
fn gpkg_binding_accepts_lowercase_epsg_authority() {
    let (_tmp, path) = fixture();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("UPDATE gpkg_spatial_ref_sys SET organization='epsg' WHERE srs_id=4326")
        .unwrap();
    conn.close().unwrap();
    assert!(read_document(&path, RecordKind::Resource).is_ok());
}

/// Schema defaults, foreign keys, and unique keys are required by Annex C/F.8.
#[test]
fn gpkg_binding_rejects_incomplete_schema() {
    for (old, new) in [
        (
            "md_scope TEXT NOT NULL DEFAULT 'dataset'",
            "md_scope TEXT NOT NULL",
        ),
        (
            "CONSTRAINT crmr_mfi_fk FOREIGN KEY (md_file_id) REFERENCES gpkg_metadata(id),",
            "",
        ),
        ("identifier TEXT UNIQUE", "identifier TEXT"),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bad.gpkg");
        let sql = include_str!("../../../tests/fixtures/gpkg/independent.sql").replace(old, new);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(&sql).unwrap();
        conn.close().unwrap();
        let before = directory_snapshot(tmp.path());
        assert!(
            matches!(
                read_document(&path, RecordKind::Resource),
                Err(MetadataError::Serialization(_))
            ),
            "{old}"
        );
        assert_eq!(directory_snapshot(tmp.path()), before);
    }
}

/// Operating-system access failures and SQLite busy conditions are operational.
#[test]
#[cfg(unix)]
fn gpkg_binding_permission_and_lock_failures_are_operational() {
    use std::os::unix::fs::PermissionsExt;
    let (_tmp, path) = fixture();
    let original = std::fs::metadata(&path).unwrap().permissions();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0)).unwrap();
    let result = read_document(&path, RecordKind::Resource);
    std::fs::set_permissions(&path, original).unwrap();
    assert!(matches!(result, Err(MetadataError::Io(_))));
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("BEGIN EXCLUSIVE").unwrap();
    assert!(matches!(
        read_document(&path, RecordKind::Resource),
        Err(MetadataError::Io(_))
    ));
    conn.execute_batch("ROLLBACK").unwrap();
    assert!(read_document(&path, RecordKind::Resource).is_ok());
}

/// GeoPackage 1.2.1 Requirement 11: reserved SRS 4326 must describe WGS 84.
#[test]
fn gpkg_binding_rejects_invalid_reserved_crs_definition() {
    for definition in ["garbage", "GEOGCS[\"WGS 84\"]", "GEOGCRS[\"WGS 84\"]"] {
        let (_tmp, path) = fixture();
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE gpkg_spatial_ref_sys SET definition=?1 WHERE srs_id=4326",
            [definition],
        )
        .unwrap();
        conn.close().unwrap();
        assert!(
            matches!(
                read_document(&path, RecordKind::Resource),
                Err(MetadataError::Serialization(_))
            ),
            "{definition}"
        );
    }
}

fn timestamp() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-09-27T12:00:00.123Z")
        .unwrap()
        .with_timezone(&chrono::Utc)
}

/// Writer output is inspected independently against OGC 12-128r15 Annex C/F.8.
#[test]
fn gpkg_binding_writer_emits_registered_metadata_extension() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("record.gpkg");
    prepare_write(
        &path,
        RecordKind::Resource,
        r#"{"ID":"writer"}"#,
        timestamp(),
    )
    .unwrap()
    .install()
    .unwrap();
    let db =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    let app: i64 = db
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 10200);
    assert_eq!(app, 0x4750_4B47);
    let registrations: i64 = db.query_row("SELECT count(*) FROM gpkg_extensions WHERE extension_name='gpkg_metadata' AND scope='read-write' AND column_name IS NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(registrations, 2);
    let rows: i64 = db.query_row("SELECT count(*) FROM gpkg_spatial_ref_sys WHERE (srs_id IN (-1,0) AND organization='NONE' AND organization_coordsys_id=srs_id AND definition='undefined') OR (srs_id=4326 AND organization='EPSG' AND organization_coordsys_id=4326)", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 3);
    let broken: i64 = db
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(broken, 0);
    let check: String = db
        .pragma_query_value(None, "quick_check", |r| r.get(0))
        .unwrap();
    assert_eq!(check, "ok");
    let (stamp, valid): (String, bool) = db.query_row("SELECT timestamp, reference_scope='geopackage' AND table_name IS NULL AND column_name IS NULL AND row_id_value IS NULL AND md_parent_id IS NULL FROM gpkg_metadata_reference", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(stamp, "2026-09-27T12:00:00.123Z");
    assert!(valid);
    let (uri, mime, json): (String, String, String) = db
        .query_row(
            "SELECT md_standard_uri, mime_type, metadata FROM gpkg_metadata",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        uri,
        "https://github.com/drtallman/OpenCDB/blob/main/docs/GPKG_METADATA.md#resource-v1"
    );
    assert_eq!(mime, "application/json");
    assert_eq!(json, r#"{"ID":"writer"}"#);
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
}

/// Replacement may not erase another document in a valid larger container.
#[test]
fn gpkg_binding_writer_refuses_extra_content_without_mutation() {
    let (tmp, path) = fixture();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("INSERT INTO gpkg_metadata SELECT 43,md_scope,md_standard_uri,mime_type,metadata FROM gpkg_metadata").unwrap();
    db.close().unwrap();
    let before = directory_snapshot(tmp.path());
    assert!(matches!(
        prepare_write(&path, RecordKind::Resource, "{}", timestamp()),
        Err(MetadataError::UnsupportedContainer { .. })
    ));
    assert_eq!(directory_snapshot(tmp.path()), before);
}

/// Preparation creates nothing; install creates parents and publishes one file.
#[test]
fn gpkg_binding_writer_prepares_before_io_and_replaces_valid_file() {
    let tmp = tempfile::tempdir().unwrap();
    let parent = tmp.path().join("new/metadata");
    let path = parent.join("record.gpkg");
    let prepared = prepare_write(&path, RecordKind::Global, "{}", timestamp()).unwrap();
    assert!(!parent.exists());
    prepared.install().unwrap();
    assert_eq!(read_document(&path, RecordKind::Global).unwrap(), "{}");
    prepare_write(
        &path,
        RecordKind::Global,
        r#"{"ID":"replacement"}"#,
        timestamp(),
    )
    .unwrap()
    .install()
    .unwrap();
    assert_eq!(
        read_document(&path, RecordKind::Global).unwrap(),
        r#"{"ID":"replacement"}"#
    );
    assert_eq!(std::fs::read_dir(parent).unwrap().count(), 1);
}

/// Invalid JSON must fail before creating any directories or replacing bytes.
#[test]
fn gpkg_binding_writer_rejects_invalid_json_before_mutation() {
    let (tmp, path) = fixture();
    let before = directory_snapshot(tmp.path());
    assert!(matches!(
        prepare_write(&path, RecordKind::Resource, "not JSON", timestamp()),
        Err(MetadataError::Serialization(_))
    ));
    assert_eq!(directory_snapshot(tmp.path()), before);
    let missing = tmp.path().join("missing/new.gpkg");
    assert!(prepare_write(&missing, RecordKind::Resource, "{", timestamp()).is_err());
    assert!(!missing.parent().unwrap().exists());
}

/// An install failure preserves old bytes and removes the temporary sibling.
#[test]
fn gpkg_binding_writer_failed_persist_preserves_old_file_and_cleans_staging() {
    let (tmp, path) = fixture();
    let before = directory_snapshot(tmp.path());
    let prepared =
        prepare_write(&path, RecordKind::Resource, r#"{"ID":"new"}"#, timestamp()).unwrap();
    assert_eq!(directory_snapshot(tmp.path()), before);
    let result = io::install_with(prepared, |file, target| {
        assert_eq!(target, path);
        assert_eq!(std::fs::read(&path).unwrap(), before[0].1);
        drop(file);
        Err(std::io::Error::other("injected persist failure"))
    });
    assert!(matches!(result, Err(MetadataError::Io(_))));
    assert_eq!(directory_snapshot(tmp.path()), before);
}

/// Replacing a metadata document preserves its access permissions.
#[test]
#[cfg(unix)]
fn gpkg_binding_writer_preserves_destination_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let (_tmp, path) = fixture();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    prepare_write(&path, RecordKind::Resource, "{}", timestamp())
        .unwrap()
        .install()
        .unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

/// Corrupt input and a different document kind must never be overwritten.
#[test]
fn gpkg_binding_writer_refuses_corrupt_or_wrong_kind_targets() {
    let (tmp, path) = fixture();
    let before = directory_snapshot(tmp.path());
    assert!(matches!(
        prepare_write(&path, RecordKind::Collection, "{}", timestamp()),
        Err(MetadataError::UnsupportedContainer { .. })
    ));
    assert_eq!(directory_snapshot(tmp.path()), before);
    std::fs::write(&path, b"corrupt input").unwrap();
    let before = directory_snapshot(tmp.path());
    assert!(matches!(
        prepare_write(&path, RecordKind::Resource, "{}", timestamp()),
        Err(MetadataError::Serialization(_))
    ));
    assert_eq!(directory_snapshot(tmp.path()), before);
}
