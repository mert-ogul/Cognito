"""Read-only graph queries via DuckDB in the sidecar."""

from __future__ import annotations

from typing import Any

import duckdb

from common.duckdb_util import ensure_schema


def snapshot(duck_path: str, limit_nodes: int) -> dict:
    ensure_schema(duck_path)
    con = duckdb.connect(duck_path)
    try:
        rows = con.execute(
            """
            SELECT node_id, name, kind, normalized_name, source_count, confidence_avg
            FROM graph_nodes
            ORDER BY source_count DESC, name ASC
            LIMIT ?
            """,
            [limit_nodes],
        ).fetchall()
        nodes = [
            {
                "nodeId": r[0],
                "name": r[1],
                "kind": r[2],
                "normalizedName": r[3],
                "sourceCount": int(r[4]),
                "confidenceAvg": float(r[5]),
            }
            for r in rows
        ]
        node_ids = {n["nodeId"] for n in nodes}
        cap = min(limit_nodes * 6, 800)
        erows = con.execute(
            """
            SELECT edge_id, source_node_id, target_node_id, relation,
                   document_id, chunk_id, confidence
            FROM graph_edges
            """
        ).fetchall()
        edges: list[dict[str, Any]] = []
        for r in erows:
            if r[1] in node_ids and r[2] in node_ids:
                edges.append(
                    {
                        "edgeId": r[0],
                        "sourceNodeId": r[1],
                        "targetNodeId": r[2],
                        "relation": r[3],
                        "documentId": r[4],
                        "chunkId": r[5],
                        "confidence": float(r[6]),
                    }
                )
                if len(edges) >= cap:
                    break
        return {"nodes": nodes, "edges": edges}
    finally:
        con.close()


def search_nodes(duck_path: str, query: str, limit: int) -> dict:
    ensure_schema(duck_path)
    con = duckdb.connect(duck_path)
    try:
        pattern = f"%{query.lower()}%"
        rows = con.execute(
            """
            SELECT node_id, name, kind, source_count FROM graph_nodes
            WHERE lower(name) LIKE ? OR lower(normalized_name) LIKE ?
            ORDER BY source_count DESC LIMIT ?
            """,
            [pattern, pattern, limit],
        ).fetchall()
        nodes = [
            {
                "nodeId": r[0],
                "name": r[1],
                "kind": r[2],
                "sourceCount": int(r[3]),
            }
            for r in rows
        ]
        return {"nodes": nodes}
    finally:
        con.close()


def node_detail(duck_path: str, node_id: str) -> dict | None:
    ensure_schema(duck_path)
    con = duckdb.connect(duck_path)
    try:
        row = con.execute(
            """
            SELECT node_id, name, kind, normalized_name, source_count, confidence_avg,
                   metadata_json, cast(created_at AS VARCHAR), cast(updated_at AS VARCHAR)
            FROM graph_nodes WHERE node_id = ?
            """,
            [node_id],
        ).fetchone()
        if row is None:
            return None
        node = {
            "nodeId": row[0],
            "name": row[1],
            "kind": row[2],
            "normalizedName": row[3],
            "sourceCount": int(row[4]),
            "confidenceAvg": float(row[5]),
            "metadataJson": row[6],
            "createdAt": row[7],
            "updatedAt": row[8],
        }
        erows = con.execute(
            """
            SELECT e.edge_id, e.source_node_id, e.target_node_id, e.relation, e.document_id,
                   e.chunk_id, e.confidence, cast(e.created_at AS VARCHAR),
                   s.name, t.name
            FROM graph_edges e
            JOIN graph_nodes s ON s.node_id = e.source_node_id
            JOIN graph_nodes t ON t.node_id = e.target_node_id
            WHERE e.source_node_id = ? OR e.target_node_id = ?
            """,
            [node_id, node_id],
        ).fetchall()
        edges = [
            {
                "edgeId": r[0],
                "sourceNodeId": r[1],
                "targetNodeId": r[2],
                "relation": r[3],
                "documentId": r[4],
                "chunkId": r[5],
                "confidence": float(r[6]),
                "createdAt": r[7],
                "sourceName": r[8],
                "targetName": r[9],
            }
            for r in erows
        ]
        return {"node": node, "edges": edges}
    finally:
        con.close()
