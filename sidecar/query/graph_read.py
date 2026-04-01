"""Read-only graph queries via DuckDB in the sidecar."""

from __future__ import annotations

import duckdb

from common.duckdb_util import ensure_schema


def snapshot(duck_path: str, limit_nodes: int) -> dict:
    ensure_schema(duck_path)
    con = duckdb.connect(duck_path)
    rows = con.execute(
        """
        SELECT node_id, name, kind, source_count FROM graph_nodes
        ORDER BY source_count DESC LIMIT ?
        """,
        [limit_nodes],
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
    cap = min(limit_nodes * 6, 800)
    erows = con.execute(
        """
        SELECT edge_id, source_node_id, target_node_id, relation FROM graph_edges LIMIT ?
        """,
        [cap],
    ).fetchall()
    edges = [
        {
            "edgeId": r[0],
            "sourceNodeId": r[1],
            "targetNodeId": r[2],
            "relation": r[3],
        }
        for r in erows
    ]
    con.close()
    return {"nodes": nodes, "edges": edges}


def search_nodes(duck_path: str, query: str, limit: int) -> dict:
    ensure_schema(duck_path)
    con = duckdb.connect(duck_path)
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
    con.close()
    return {"nodes": nodes}
