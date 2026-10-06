//! Migration test (plan §6.3): every shipped schema version's fixture migrates to
//! the current version with no data loss, checked row by row.

use linen_lib::store::{Store, MIGRATIONS};
use rusqlite::Connection;
use std::path::PathBuf;

const TABLES: &[&str] = &[
    "books",
    "book_damage",
    "positions",
    "book_settings",
    "annotations",
    "settings",
    "extensions",
    "extension_storage",
    "search_text",
    "sync_records",
    "sync_outbox",
    "sync_aside",
];

fn dump(conn: &Connection, table: &str) -> Vec<Vec<String>> {
    let mut stmt = conn
        .prepare(&format!("SELECT * FROM {table} ORDER BY 1, 2"))
        .unwrap();
    let cols = stmt.column_count();
    stmt.query_map([], |r| {
        Ok((0..cols)
            .map(|i| format!("{:?}", r.get_ref(i).unwrap()))
            .collect::<Vec<_>>())
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

fn columns(conn: &Connection) -> Vec<(String, Vec<String>)> {
    TABLES
        .iter()
        .map(|t| {
            let mut stmt = conn.prepare(&format!("PRAGMA table_info({t})")).unwrap();
            let cols = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            (t.to_string(), cols)
        })
        .collect()
}

/// Each fixture must have exactly the schema the migrations produce at its version.
#[test]
fn fixtures_match_the_migrations() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for version in 1..=MIGRATIONS.len() {
        let fixture = Connection::open_in_memory().unwrap();
        fixture
            .execute_batch(
                &std::fs::read_to_string(dir.join(format!("schema-v{version}.sql"))).unwrap(),
            )
            .unwrap();
        let migrated = Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..version] {
            migrated.execute_batch(sql).unwrap();
        }
        assert_eq!(
            columns(&fixture),
            columns(&migrated),
            "schema-v{version}.sql is out of date"
        );
    }
}

#[test]
fn every_fixture_migrates_without_data_loss() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for version in 1..=MIGRATIONS.len() {
        let fixture = dir.join(format!("schema-v{version}.sql"));
        let sql = std::fs::read_to_string(&fixture).unwrap_or_else(|_| {
            panic!(
                "missing fixture for shipped schema version {version}: {}",
                fixture.display()
            )
        });
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("linen.db");
        // Tables that exist at this version (later versions add some).
        let (tables, before): (Vec<&str>, Vec<_>) = {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(&sql).unwrap();
            let tables: Vec<&str> = TABLES
                .iter()
                .copied()
                .filter(|t| {
                    c.query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                        [t],
                        |r| r.get::<_, i64>(0),
                    )
                    .unwrap()
                        == 1
                })
                .collect();
            let rows = tables.iter().map(|t| dump(&c, t)).collect();
            (tables, rows)
        };
        assert!(
            before.iter().all(|rows| !rows.is_empty()),
            "v{version} fixture must have rows in every table"
        );

        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), MIGRATIONS.len() as i64);
        for (table, rows) in tables.iter().zip(&before) {
            let after = dump(store.conn(), table);
            // Later migrations may add columns; the original columns must be unchanged.
            for (old, new) in rows.iter().zip(&after) {
                assert_eq!(&new[..old.len()], &old[..], "v{version} {table}");
            }
            assert_eq!(rows.len(), after.len(), "v{version} {table} row count");
        }
    }
}
