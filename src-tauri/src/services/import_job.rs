//! Import job row lifecycle (SQLite).

use crate::db::sqlite;
use crate::errors::CognitoResult;
use rusqlite::Connection;

pub fn start_job(conn: &Connection, id: &str, total: i64) -> CognitoResult<()> {
    sqlite::insert_import_job(conn, id, total)
}

pub fn tick(conn: &Connection, id: &str, done: i64, failed: i64) -> CognitoResult<()> {
    sqlite::update_import_job_counts(conn, id, done, failed)
}

pub fn complete_ok(conn: &Connection, id: &str) -> CognitoResult<()> {
    sqlite::finish_import_job(conn, id, "completed", None)
}

pub fn complete_fail(conn: &Connection, id: &str, msg: &str) -> CognitoResult<()> {
    sqlite::finish_import_job(conn, id, "failed", Some(msg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::sqlite::open_sqlite;
    use std::fs;

    #[test]
    fn job_transitions_running_to_completed() {
        let dir = tempfile::tempdir().expect("d");
        let p = dir.path().join("j.sqlite");
        {
            let c = open_sqlite(&p).expect("open");
            start_job(&c, "j1", 5).expect("start");
            tick(&c, "j1", 3, 1).expect("tick");
            let d: i64 = c
                .query_row(
                    "SELECT completed_files FROM import_jobs WHERE id='j1'",
                    [],
                    |r| r.get(0),
                )
                .expect("q");
            assert_eq!(d, 3);
            complete_ok(&c, "j1").expect("done");
            let s: String = c
                .query_row("SELECT status FROM import_jobs WHERE id='j1'", [], |r| r.get(0))
                .expect("s");
            assert_eq!(s, "completed");
        }
        let _ = fs::remove_file(&p);
    }
}
