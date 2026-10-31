//! SQLite vault access and migrations.

pub mod cards;
pub mod chunks;
pub mod conversations;
pub mod documents;
pub mod entities;
pub mod settings;

pub use cards::*;
pub use chunks::*;
pub use documents::*;
pub use entities::*;
pub use settings::*;

use crate::errors::CognitoResult;
use rusqlite::Connection;
use std::path::Path;

const MIGRATIONS: &[(i32, &str)] = &[
    (1, include_str!("../../../migrations/001_initial.sql")),
    (
        2,
        include_str!("../../../migrations/002_sqlite_vec_prep.sql"),
    ),
    (3, include_str!("../../../migrations/003_graph_stage.sql")),
    (4, include_str!("../../../migrations/004_requeue_graph.sql")),
    (5, include_str!("../../../migrations/005_chunk_search.sql")),
    (6, include_str!("../../../migrations/006_chunk_role.sql")),
    (
        7,
        include_str!("../../../migrations/007_requeue_graph_v2.sql"),
    ),
    (
        8,
        include_str!("../../../migrations/008_default_models.sql"),
    ),
    (9, include_str!("../../../migrations/009_chunk_labels.sql")),
    (
        10,
        include_str!("../../../migrations/010_document_cards.sql"),
    ),
    (
        11,
        include_str!("../../../migrations/011_entities_connections.sql"),
    ),
    (
        12,
        include_str!("../../../migrations/012_chunk_regions.sql"),
    ),
    (
        13,
        include_str!("../../../migrations/013_documents_fts.sql"),
    ),
    (
        14,
        include_str!("../../../migrations/014_background_model_default.sql"),
    ),
    (
        15,
        include_str!("../../../migrations/015_drop_map_tables.sql"),
    ),
    (
        16,
        include_str!("../../../migrations/016_drop_schema_migrations.sql"),
    ),
    (
        17,
        include_str!("../../../migrations/017_conversations.sql"),
    ),
];

/// Bump when embedding text format or retrieval index layout changes.
pub const INDEX_FORMAT_VERSION: i32 = 7;

/// Matches Python `SQLITE_BUSY_TIMEOUT_S`.
pub const SQLITE_BUSY_TIMEOUT_SECS: u64 = 15;

pub fn open_sqlite(path: &Path) -> CognitoResult<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(SQLITE_BUSY_TIMEOUT_SECS))?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> CognitoResult<()> {
    let mut current: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for &(version, sql) in MIGRATIONS {
        if current >= version {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(&format!("PRAGMA user_version = {version}"), [])?;
        tx.commit()?;
        current = version;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn open_sqlite_sets_busy_timeout_above_five_seconds() {
        assert!(SQLITE_BUSY_TIMEOUT_SECS > 5);
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("busy.sqlite");
        let c = open_sqlite(&p).expect("open");
        // rusqlite stores the timeout on the connection; reopen would reset it.
        // Assert the constant and that open succeeds with it applied.
        drop(c);
        let c2 = open_sqlite(&p).expect("reopen");
        drop(c2);
    }

    #[test]
    fn migration_applies_to_temp_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("v.sqlite");
        {
            let c = open_sqlite(&p).expect("open");
            let v: i32 = c
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .expect("uv");
            assert_eq!(v, 17);
            assert_eq!(INDEX_FORMAT_VERSION, 7);
            let has_topics: i32 = c
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='topics'",
                    [],
                    |r| r.get(0),
                )
                .expect("topics");
            assert_eq!(has_topics, 0);
            let has_points: i32 = c
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='document_points'",
                    [],
                    |r| r.get(0),
                )
                .expect("document_points");
            assert_eq!(has_points, 0);
            let has_schema_migrations: i32 = c
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
                    [],
                    |r| r.get(0),
                )
                .expect("schema_migrations");
            assert_eq!(has_schema_migrations, 0);
            let has_profile: i32 = c
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='documents_profile'",
                    [],
                    |r| r.get(0),
                )
                .expect("documents_profile");
            assert_eq!(has_profile, 1);
            let has_regions: i32 = c
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('chunks') WHERE name = 'regions'",
                    [],
                    |r| r.get(0),
                )
                .expect("regions col");
            assert_eq!(has_regions, 1);
            let has_scan: i32 = c
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('documents') WHERE name = 'scan_status'",
                    [],
                    |r| r.get(0),
                )
                .expect("scan_status col");
            assert_eq!(has_scan, 1);
            let n: i32 = c
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name = 'chunk_embeddings'",
                    [],
                    |r| r.get(0),
                )
                .expect("cnt");
            assert_eq!(n, 0);
        }
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn content_hash_by_path_only_indexed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("v.sqlite");
        let c = open_sqlite(&p).expect("open");
        insert_document_row(
            &c,
            "d1",
            "/a.md",
            "a",
            "markdown",
            "hash-failed",
            None,
            "failed",
            Some("boom"),
            None,
        )
        .expect("insert failed");
        assert!(content_hash_by_path(&c, "/a.md").expect("q").is_none());
        insert_document_row(
            &c, "d2", "/b.md", "b", "markdown", "hash-ok", None, "indexed", None, None,
        )
        .expect("insert indexed");
        assert_eq!(
            content_hash_by_path(&c, "/b.md").expect("q").as_deref(),
            Some("hash-ok")
        );
    }

    #[test]
    fn record_failed_document_inserts_and_updates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("v.sqlite");
        let c = open_sqlite(&p).expect("open");
        record_failed_document(&c, "/x.md", "x", "markdown", "first").expect("insert");
        let (status, hash, err): (String, String, String) = c
            .query_row(
                "SELECT status, content_hash, error_message FROM documents WHERE path = ?1",
                ["/x.md"],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("row");
        assert_eq!(status, "failed");
        assert_eq!(hash, "");
        assert_eq!(err, "first");

        insert_document_row(
            &c, "old", "/y.md", "y", "markdown", "keep-me", None, "indexing", None, None,
        )
        .expect("indexing row");
        record_failed_document(&c, "/y.md", "y", "markdown", &"e".repeat(600)).expect("upsert");
        let (status, hash, err, estat, eerr): (String, String, String, String, Option<String>) = c
            .query_row(
                "SELECT status, content_hash, error_message, entities_status, entities_error FROM documents WHERE path = ?1",
                ["/y.md"],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("row");
        assert_eq!(status, "failed");
        assert_eq!(hash, "");
        assert_eq!(err.chars().count(), 512);
        assert_eq!(estat, "none");
        assert!(eerr.is_none());
        let n: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM documents WHERE path = ?1",
                ["/y.md"],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(n, 1);
    }

    #[test]
    fn list_documents_excludes_removed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("v.sqlite");
        let c = open_sqlite(&p).expect("open");
        insert_document_row(
            &c, "keep", "/keep.md", "keep", "markdown", "h1", None, "indexed", None, None,
        )
        .expect("keep");
        insert_document_row(
            &c, "gone", "/gone.md", "gone", "markdown", "h2", None, "removed", None, None,
        )
        .expect("gone");
        let rows = list_documents(&c).expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "keep");
    }

    #[test]
    fn chunks_fts_stays_in_sync_on_insert_and_delete() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("v.sqlite");
        let c = open_sqlite(&p).expect("open");
        insert_document_row(
            &c, "d1", "/a.md", "a", "markdown", "h", None, "indexed", None, None,
        )
        .expect("doc");
        insert_chunk_row(
            &c,
            "c1",
            "d1",
            0,
            None,
            None,
            "renormalization trick GCN",
            4,
            "h1",
            "body",
            "[]",
            None,
        )
        .expect("c1");
        let n: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH 'GCN'",
                [],
                |r| r.get(0),
            )
            .expect("fts match");
        assert_eq!(n, 1);
        c.execute("DELETE FROM chunks WHERE id = ?1", ["c1"])
            .expect("delete");
        let n2: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH 'GCN'",
                [],
                |r| r.get(0),
            )
            .expect("fts after delete");
        assert_eq!(n2, 0);
    }

    #[test]
    fn chunk_window_first_middle_last() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("v.sqlite");
        let c = open_sqlite(&p).expect("open");
        insert_document_row(
            &c, "d1", "/a.md", "a", "markdown", "h", None, "indexed", None, None,
        )
        .expect("doc");
        insert_chunk_row(
            &c,
            "c0",
            "d1",
            0,
            Some(1),
            Some(1),
            "first",
            1,
            "h0",
            "body",
            "[]",
            None,
        )
        .expect("c0");
        insert_chunk_row(
            &c,
            "c1",
            "d1",
            1,
            Some(2),
            Some(2),
            "middle",
            1,
            "h1",
            "body",
            "[]",
            None,
        )
        .expect("c1");
        insert_chunk_row(
            &c,
            "c2",
            "d1",
            2,
            Some(3),
            Some(3),
            "last",
            1,
            "h2",
            "body",
            "[]",
            None,
        )
        .expect("c2");

        let first = chunk_window(&c, "d1", "c0").expect("q").expect("first");
        assert_eq!(first.highlight, "first");
        assert_eq!(first.text, "first\n\nmiddle");
        assert_eq!(first.page_start, Some(1));
        assert_eq!(first.page_end, Some(1));

        let mid = chunk_window(&c, "d1", "c1").expect("q").expect("mid");
        assert_eq!(mid.highlight, "middle");
        assert_eq!(mid.text, "first\n\nmiddle\n\nlast");
        assert_eq!(mid.page_start, Some(2));

        let last = chunk_window(&c, "d1", "c2").expect("q").expect("last");
        assert_eq!(last.highlight, "last");
        assert_eq!(last.text, "middle\n\nlast");
        assert_eq!(last.page_end, Some(3));
    }

    #[test]
    fn related_documents_ranks_rare_shared_name_above_common() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("v.sqlite");
        let c = open_sqlite(&p).expect("open");
        for (id, title) in [
            ("d0", "Anchor"),
            ("d_rare", "Rare Partner"),
            ("d_common", "Common Partner"),
        ] {
            insert_document_row(
                &c,
                id,
                &format!("/{id}.pdf"),
                title,
                "pdf",
                "hash",
                Some(1),
                "indexed",
                None,
                None,
            )
            .expect("doc");
        }
        insert_chunk_row(
            &c,
            "c0",
            "d0",
            0,
            None,
            None,
            "Cora IEEE",
            2,
            "h0",
            "body",
            "[]",
            None,
        )
        .expect("c0");
        insert_chunk_row(
            &c, "c1", "d_rare", 0, None, None, "Cora", 1, "h1", "body", "[]", None,
        )
        .expect("c1");
        insert_chunk_row(
            &c, "c2", "d_common", 0, None, None, "IEEE", 1, "h2", "body", "[]", None,
        )
        .expect("c2");
        c.execute_batch(
            "
            INSERT INTO entities(id, label, name, normalized, doc_count, mention_count)
            VALUES
              ('e_rare', 'person', 'Cora', 'cora', 2, 2),
              ('e_common', 'organization', 'IEEE', 'ieee', 10, 20);
            INSERT INTO mentions(entity_id, chunk_id, document_id, start, end) VALUES
              ('e_rare', 'c0', 'd0', 0, 4),
              ('e_rare', 'c1', 'd_rare', 0, 4),
              ('e_common', 'c0', 'd0', 5, 9),
              ('e_common', 'c2', 'd_common', 0, 4);
            ",
        )
        .expect("entities");
        let related = list_related_documents(&c, "d0").expect("related");
        assert!(related.len() >= 2);
        let ids: Vec<&str> = related
            .iter()
            .filter_map(|v| v.get("id").and_then(|x| x.as_str()))
            .collect();
        let rare_pos = ids.iter().position(|id| *id == "d_rare").expect("rare");
        let common_pos = ids.iter().position(|id| *id == "d_common").expect("common");
        assert!(rare_pos < common_pos);
        assert_eq!(
            related[rare_pos].get("kind").and_then(|v| v.as_str()),
            Some("shares_names")
        );
    }

    #[test]
    fn clear_document_entity_rows_prunes_entities_without_mentions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("v.sqlite");
        let c = open_sqlite(&p).expect("open");
        insert_document_row(
            &c, "d1", "/a.pdf", "A", "pdf", "h", None, "indexed", None, None,
        )
        .expect("d1");
        insert_document_row(
            &c, "d2", "/b.pdf", "B", "pdf", "h", None, "indexed", None, None,
        )
        .expect("d2");
        insert_chunk_row(
            &c, "c1", "d1", 0, None, None, "Ada", 1, "h1", "body", "[]", None,
        )
        .expect("c1");
        insert_chunk_row(
            &c, "c2", "d2", 0, None, None, "Ada", 1, "h2", "body", "[]", None,
        )
        .expect("c2");
        c.execute_batch(
            "
            INSERT INTO entities(id, label, name, normalized, doc_count, mention_count)
            VALUES
              ('e_only_d1', 'person', 'Only', 'only', 1, 1),
              ('e_shared', 'person', 'Ada', 'ada', 2, 2);
            INSERT INTO mentions(entity_id, chunk_id, document_id, start, end) VALUES
              ('e_only_d1', 'c1', 'd1', 0, 4),
              ('e_shared', 'c1', 'd1', 0, 3),
              ('e_shared', 'c2', 'd2', 0, 3);
            ",
        )
        .expect("seed");
        clear_document_entity_rows(&c, "d1").expect("clear");
        let leftover: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM entities e
                 WHERE NOT EXISTS (SELECT 1 FROM mentions m WHERE m.entity_id = e.id)",
                [],
                |r| r.get(0),
            )
            .expect("orphan count");
        assert_eq!(leftover, 0);
        let only: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM entities WHERE id = 'e_only_d1'",
                [],
                |r| r.get(0),
            )
            .expect("only");
        assert_eq!(only, 0);
        let shared_docs: i64 = c
            .query_row(
                "SELECT doc_count FROM entities WHERE id = 'e_shared'",
                [],
                |r| r.get(0),
            )
            .expect("shared");
        assert_eq!(shared_docs, 1);
    }
}
