//! Read-only graph queries over DuckDB adjacency tables.

use crate::errors::{CognitoError, CognitoResult};
use duckdb::Connection;
use serde_json::{json, Value};
use std::collections::HashSet;

/// Lookup a node by exact normalized name.
pub fn node_by_normalized_name(conn: &Connection, normalized: &str) -> CognitoResult<Option<Value>> {
    let mut stmt = conn
        .prepare(
            "SELECT node_id, name, kind, normalized_name, source_count, confidence_avg, metadata_json,
                    cast(created_at as VARCHAR) AS created_at, cast(updated_at as VARCHAR) AS updated_at
             FROM graph_nodes WHERE normalized_name = ? LIMIT 1",
        )
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let mut rows = stmt
        .query_map([normalized], |r| {
            let meta: Option<String> = r.get(6)?;
            Ok(json!({
                "nodeId": r.get::<_, String>(0)?,
                "name": r.get::<_, String>(1)?,
                "kind": r.get::<_, String>(2)?,
                "normalizedName": r.get::<_, String>(3)?,
                "sourceCount": r.get::<_, i64>(4)?,
                "confidenceAvg": r.get::<_, f64>(5)?,
                "metadataJson": meta,
                "createdAt": r.get::<_, String>(7)?,
                "updatedAt": r.get::<_, String>(8)?,
            }))
        })
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    if let Some(r) = rows.next() {
        return Ok(Some(r.map_err(|e| CognitoError::Database(e.to_string()))?));
    }
    Ok(None)
}

fn undirected_neighbors(conn: &Connection, nid: &str) -> CognitoResult<HashSet<String>> {
    let mut stmt = conn
        .prepare(
            "SELECT CASE WHEN source_node_id = ? THEN target_node_id ELSE source_node_id END
             FROM graph_edges WHERE source_node_id = ? OR target_node_id = ?",
        )
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let rows = stmt
        .query_map(duckdb::params![nid, nid, nid], |r| r.get::<_, String>(0))
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let mut s = HashSet::new();
    for x in rows {
        s.insert(x.map_err(|e| CognitoError::Database(e.to_string()))?);
    }
    Ok(s)
}

/// Undirected neighborhood up to depth 2 from a seed `node_id` (for visualization / exploration).
pub fn neighbors_two_hop(conn: &Connection, seed_node_id: &str) -> CognitoResult<Value> {
    let h1 = undirected_neighbors(conn, seed_node_id)?;
    let mut h2: HashSet<String> = HashSet::new();
    for n in &h1 {
        let sub = undirected_neighbors(conn, n)?;
        h2.extend(sub);
    }
    h2.remove(seed_node_id);
    for x in &h1 {
        h2.remove(x);
    }

    let mut node_ids: HashSet<String> = HashSet::new();
    node_ids.insert(seed_node_id.to_string());
    node_ids.extend(h1.iter().cloned());
    node_ids.extend(h2.iter().cloned());

    let mut nodes = Vec::new();
    for nid in &node_ids {
        let mut stmt = conn
            .prepare(
                "SELECT node_id, name, kind, source_count FROM graph_nodes WHERE node_id = ?",
            )
            .map_err(|e| CognitoError::Database(e.to_string()))?;
        let mut rows = stmt
            .query_map([nid], |r| {
                Ok(json!({
                    "nodeId": r.get::<_, String>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "kind": r.get::<_, String>(2)?,
                    "sourceCount": r.get::<_, i64>(3)?,
                }))
            })
            .map_err(|e| CognitoError::Database(e.to_string()))?;
        if let Some(Ok(v)) = rows.next() {
            nodes.push(v);
        }
    }

    let idset = node_ids;
    let mut estmt = conn
        .prepare(
            "SELECT edge_id, source_node_id, target_node_id, relation, document_id, chunk_id, confidence FROM graph_edges",
        )
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let rows = estmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, f64>(6)?,
            ))
        })
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let mut edges = Vec::new();
    for row in rows.flatten() {
        let (eid, s, t, rel, doc, ch, conf) = row;
        if idset.contains(&s) && idset.contains(&t) {
            edges.push(json!({
                "edgeId": eid,
                "sourceNodeId": s,
                "targetNodeId": t,
                "relation": rel,
                "documentId": doc,
                "chunkId": ch,
                "confidence": conf,
            }));
        }
    }

    Ok(json!({ "nodes": nodes, "edges": edges }))
}

pub fn edges_for_document(conn: &Connection, document_id: &str) -> CognitoResult<Vec<Value>> {
    let mut stmt = conn
        .prepare(
            "SELECT edge_id, source_node_id, target_node_id, relation, chunk_id, confidence,
                    cast(created_at as VARCHAR) AS created_at
             FROM graph_edges WHERE document_id = ? ORDER BY created_at",
        )
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let rows = stmt
        .query_map([document_id], |r| {
            Ok(json!({
                "edgeId": r.get::<_, String>(0)?,
                "sourceNodeId": r.get::<_, String>(1)?,
                "targetNodeId": r.get::<_, String>(2)?,
                "relation": r.get::<_, String>(3)?,
                "chunkId": r.get::<_, String>(4)?,
                "confidence": r.get::<_, f64>(5)?,
                "createdAt": r.get::<_, String>(6)?,
            }))
        })
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| CognitoError::Database(e.to_string()))?);
    }
    Ok(out)
}

pub fn edges_for_chunk(conn: &Connection, chunk_id: &str) -> CognitoResult<Vec<Value>> {
    let mut stmt = conn
        .prepare(
            "SELECT edge_id, source_node_id, target_node_id, relation, document_id, confidence,
                    cast(created_at as VARCHAR) AS created_at
             FROM graph_edges WHERE chunk_id = ? ORDER BY created_at",
        )
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let rows = stmt
        .query_map([chunk_id], |r| {
            Ok(json!({
                "edgeId": r.get::<_, String>(0)?,
                "sourceNodeId": r.get::<_, String>(1)?,
                "targetNodeId": r.get::<_, String>(2)?,
                "relation": r.get::<_, String>(3)?,
                "documentId": r.get::<_, String>(4)?,
                "confidence": r.get::<_, f64>(5)?,
                "createdAt": r.get::<_, String>(6)?,
            }))
        })
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| CognitoError::Database(e.to_string()))?);
    }
    Ok(out)
}

/// UI snapshot: top nodes by salience and a capped edge list.
pub fn graph_snapshot(conn: &Connection, limit_nodes: usize, edge_cap: usize) -> CognitoResult<Value> {
    let mut stmt = conn
        .prepare(
            "SELECT node_id, name, kind, normalized_name, source_count, confidence_avg
             FROM graph_nodes ORDER BY source_count DESC, name ASC LIMIT ?",
        )
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let rows = stmt
        .query_map([limit_nodes as i64], |r| {
            Ok(json!({
                "nodeId": r.get::<_, String>(0)?,
                "name": r.get::<_, String>(1)?,
                "kind": r.get::<_, String>(2)?,
                "normalizedName": r.get::<_, String>(3)?,
                "sourceCount": r.get::<_, i64>(4)?,
                "confidenceAvg": r.get::<_, f64>(5)?,
            }))
        })
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let nodes: Vec<Value> = rows.filter_map(|x| x.ok()).collect();

    let mut estmt = conn
        .prepare(
            "SELECT edge_id, source_node_id, target_node_id, relation, document_id, chunk_id, confidence
             FROM graph_edges LIMIT ?",
        )
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let erows = estmt
        .query_map([edge_cap as i64], |r| {
            Ok(json!({
                "edgeId": r.get::<_, String>(0)?,
                "sourceNodeId": r.get::<_, String>(1)?,
                "targetNodeId": r.get::<_, String>(2)?,
                "relation": r.get::<_, String>(3)?,
                "documentId": r.get::<_, String>(4)?,
                "chunkId": r.get::<_, String>(5)?,
                "confidence": r.get::<_, f64>(6)?,
            }))
        })
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let edges: Vec<Value> = erows.filter_map(|x| x.ok()).collect();

    Ok(json!({ "nodes": nodes, "edges": edges }))
}

pub fn node_detail(conn: &Connection, node_id: &str) -> CognitoResult<Option<Value>> {
    let mut stmt = conn
        .prepare(
            "SELECT node_id, name, kind, normalized_name, source_count, confidence_avg, metadata_json,
                    cast(created_at as VARCHAR) AS created_at, cast(updated_at as VARCHAR) AS updated_at
             FROM graph_nodes WHERE node_id = ?",
        )
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let mut rows = stmt
        .query_map([node_id], |r| {
            let meta: Option<String> = r.get(6)?;
            Ok(json!({
                "nodeId": r.get::<_, String>(0)?,
                "name": r.get::<_, String>(1)?,
                "kind": r.get::<_, String>(2)?,
                "normalizedName": r.get::<_, String>(3)?,
                "sourceCount": r.get::<_, i64>(4)?,
                "confidenceAvg": r.get::<_, f64>(5)?,
                "metadataJson": meta,
                "createdAt": r.get::<_, String>(7)?,
                "updatedAt": r.get::<_, String>(8)?,
            }))
        })
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let Some(Ok(node)) = rows.next() else {
        return Ok(None);
    };
    let mut estmt = conn
        .prepare(
            "SELECT edge_id, source_node_id, target_node_id, relation, document_id, chunk_id, confidence,
                    cast(created_at as VARCHAR) AS created_at
             FROM graph_edges WHERE source_node_id = ? OR target_node_id = ?",
        )
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let erows = estmt
        .query_map(duckdb::params![node_id, node_id], |r| {
            Ok(json!({
                "edgeId": r.get::<_, String>(0)?,
                "sourceNodeId": r.get::<_, String>(1)?,
                "targetNodeId": r.get::<_, String>(2)?,
                "relation": r.get::<_, String>(3)?,
                "documentId": r.get::<_, String>(4)?,
                "chunkId": r.get::<_, String>(5)?,
                "confidence": r.get::<_, f64>(6)?,
                "createdAt": r.get::<_, String>(7)?,
            }))
        })
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let edges: Vec<Value> = erows.filter_map(|x| x.ok()).collect();
    Ok(Some(json!({ "node": node, "edges": edges })))
}

pub fn search_nodes(conn: &Connection, query: &str, limit: usize) -> CognitoResult<Vec<Value>> {
    let pattern = format!("%{}%", query.to_lowercase());
    let mut stmt = conn
        .prepare(
            "SELECT node_id, name, kind, source_count FROM graph_nodes
             WHERE lower(name) LIKE ? OR lower(normalized_name) LIKE ?
             ORDER BY source_count DESC LIMIT ?",
        )
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let rows = stmt
        .query_map(
            duckdb::params![pattern.clone(), pattern, limit as i64],
            |r| {
                Ok(json!({
                    "nodeId": r.get::<_, String>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "kind": r.get::<_, String>(2)?,
                    "sourceCount": r.get::<_, i64>(3)?,
                }))
            },
        )
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| CognitoError::Database(e.to_string()))?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::duckdb::open_graph_store;
    use std::fs;

    fn seed_chain(conn: &Connection) {
        conn.execute_batch(
            r"
            INSERT INTO graph_nodes (node_id, name, kind, normalized_name, source_count, confidence_avg, metadata_json, created_at, updated_at)
            VALUES
              ('n_a', 'Alice', 'person', 'alice', 1, 0.9, '{}', now(), now()),
              ('n_b', 'Bob', 'person', 'bob', 1, 0.9, '{}', now(), now()),
              ('n_c', 'Charlie', 'person', 'charlie', 1, 0.9, '{}', now(), now());
            INSERT INTO graph_edges (edge_id, source_node_id, target_node_id, relation, document_id, chunk_id, confidence, created_at)
            VALUES
              ('e1', 'n_a', 'n_b', 'knows', 'doc1', 'c1', 0.9, now()),
              ('e2', 'n_b', 'n_c', 'knows', 'doc1', 'c2', 0.8, now());
            ",
        )
        .expect("seed");
    }

    #[test]
    fn node_by_normalized_name_finds_row() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("g.duckdb");
        let conn = open_graph_store(&p).expect("open");
        seed_chain(&conn);
        let v = node_by_normalized_name(&conn, "alice")
            .expect("q")
            .expect("some");
        assert_eq!(v["nodeId"], "n_a");
        drop(conn);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn neighbors_two_hop_includes_chain() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("g2.duckdb");
        let conn = open_graph_store(&p).expect("open");
        seed_chain(&conn);
        let v = neighbors_two_hop(&conn, "n_a").expect("n2");
        let nodes = v["nodes"].as_array().expect("nodes");
        let ids: Vec<&str> = nodes
            .iter()
            .filter_map(|n| n["nodeId"].as_str())
            .collect();
        assert!(ids.contains(&"n_a"));
        assert!(ids.contains(&"n_b"));
        assert!(ids.contains(&"n_c"));
        drop(conn);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn edges_for_document_and_chunk() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("g3.duckdb");
        let conn = open_graph_store(&p).expect("open");
        seed_chain(&conn);
        let d = edges_for_document(&conn, "doc1").expect("ed");
        assert_eq!(d.len(), 2);
        let c = edges_for_chunk(&conn, "c1").expect("ec");
        assert_eq!(c.len(), 1);
        drop(conn);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn graph_snapshot_non_empty() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("g4.duckdb");
        let conn = open_graph_store(&p).expect("open");
        seed_chain(&conn);
        let v = graph_snapshot(&conn, 10, 20).expect("snap");
        assert!(!v["nodes"].as_array().unwrap().is_empty());
        drop(conn);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn node_detail_has_edges() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("g5.duckdb");
        let conn = open_graph_store(&p).expect("open");
        seed_chain(&conn);
        let d = node_detail(&conn, "n_b").expect("nd").expect("row");
        assert!(d["edges"].as_array().unwrap().len() >= 1);
        drop(conn);
        let _ = fs::remove_file(&p);
    }
}
