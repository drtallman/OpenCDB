-- Independent transcription: OGC 12-128r15 Annex C.1/C.2/C.8 and F.8.
CREATE TABLE gpkg_spatial_ref_sys (
 srs_name TEXT NOT NULL, srs_id INTEGER NOT NULL PRIMARY KEY,
 organization TEXT NOT NULL, organization_coordsys_id INTEGER NOT NULL,
 definition TEXT NOT NULL, description TEXT
);
CREATE TABLE gpkg_contents (
 table_name TEXT NOT NULL PRIMARY KEY, data_type TEXT NOT NULL,
 identifier TEXT UNIQUE, description TEXT DEFAULT '',
 last_change DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 min_x DOUBLE, min_y DOUBLE, max_x DOUBLE, max_y DOUBLE, srs_id INTEGER,
 CONSTRAINT fk_gc_r_srs_id FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys(srs_id)
);
CREATE TABLE gpkg_extensions (
 table_name TEXT, column_name TEXT, extension_name TEXT NOT NULL,
 definition TEXT NOT NULL, scope TEXT NOT NULL,
 CONSTRAINT ge_tce UNIQUE (table_name, column_name, extension_name)
);
CREATE TABLE gpkg_metadata (
 id INTEGER CONSTRAINT m_pk PRIMARY KEY ASC NOT NULL,
 md_scope TEXT NOT NULL DEFAULT 'dataset', md_standard_uri TEXT NOT NULL,
 mime_type TEXT NOT NULL DEFAULT 'text/xml', metadata TEXT NOT NULL DEFAULT ''
);
CREATE TABLE gpkg_metadata_reference (
 reference_scope TEXT NOT NULL, table_name TEXT, column_name TEXT,
 row_id_value INTEGER,
 timestamp DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 md_file_id INTEGER NOT NULL, md_parent_id INTEGER,
 CONSTRAINT crmr_mfi_fk FOREIGN KEY (md_file_id) REFERENCES gpkg_metadata(id),
 CONSTRAINT crmr_mpi_fk FOREIGN KEY (md_parent_id) REFERENCES gpkg_metadata(id)
);
INSERT INTO gpkg_spatial_ref_sys VALUES
 ('Undefined Cartesian', -1, 'NONE', -1, 'undefined', 'Undefined Cartesian coordinate reference system'),
 ('Undefined Geographic', 0, 'NONE', 0, 'undefined', 'Undefined geographic coordinate reference system'),
 ('WGS 84 geodetic', 4326, 'EPSG', 4326, 'GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433],AUTHORITY["EPSG","4326"]]', 'longitude/latitude coordinates in decimal degrees on the WGS 84 spheroid');
PRAGMA application_id = 1196444487;
PRAGMA user_version = 10200;
INSERT INTO gpkg_extensions VALUES
 ('gpkg_metadata', NULL, 'gpkg_metadata', 'http://www.geopackage.org/spec121/#extension_metadata', 'read-write'),
 ('gpkg_metadata_reference', NULL, 'gpkg_metadata', 'http://www.geopackage.org/spec121/#extension_metadata', 'read-write');
INSERT INTO gpkg_metadata (id, md_scope, md_standard_uri, mime_type, metadata)
VALUES (42, 'dataset', 'https://github.com/drtallman/OpenCDB/blob/main/docs/GPKG_METADATA.md#resource-v1',
 'application/json', '{"ID":"independent","type":"dataset","title":"Independent","description":"SQL fixture"}');
INSERT INTO gpkg_metadata_reference
 (reference_scope, table_name, column_name, row_id_value, timestamp, md_file_id, md_parent_id)
VALUES ('geopackage', NULL, NULL, NULL, '2026-09-27T12:00:00.000Z', 42, NULL);
