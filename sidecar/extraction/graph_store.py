"""DuckDB merge helpers for graph_nodes / graph_edges."""

from __future__ import annotations

import hashlib
import json
from datetime import datetime, timezone
from typing import Any

import duckdb

from common.duckdb_util import ensure_schema
from extraction.extraction_result import ExtractedEdge, ExtractedEntity


def node_id_from_normalized(normalized: str) -> str:
    h = hashlib.sha256(normalized.encode("utf-8")).hexdigest()[:48]
    return f"n_{h}"


def edge_id_deterministic(
    document_id: str, chunk_id: str, src_norm: str, tgt_norm: str, relation: str
) -> str:
    raw = f"{document_id}|{chunk_id}|{src_norm}|{tgt_norm}|{relation}"
    h = hashlib.sha256(raw.encode("utf-8")).hexdigest()[:48]
    return f"e_{h}"


def _utc_now() -> datetime:
    return datetime.now(timezone.utc)


def purge_document_graph(duck_path: str, document_id: str) -> None:
    """Remove graph data for a document before re-extraction; cleans orphan nodes."""
    ensure_schema(duck_path)
    con = duckdb.connect(duck_path)
    try:
        con.execute("DELETE FROM graph_edges WHERE document_id = ?", [document_id])
        rows = con.execute("SELECT node_id, metadata_json FROM graph_nodes").fetchall()
        now = _utc_now()
        for nid, mj in rows:
            md = json.loads(mj or "{}")
            ev = [e for e in md.get("evidence", []) if e.get("document_id") != document_id]
            if len(ev) == len(md.get("evidence", [])):
                continue
            md["evidence"] = ev
            confs = [float(e.get("confidence", 0.8)) for e in ev]
            sc = len(ev)
            avg = sum(confs) / max(sc, 1) if ev else 0.0
            still = con.execute(
                """
                SELECT 1 FROM graph_edges
                WHERE source_node_id = ? OR target_node_id = ? LIMIT 1
                """,
                [nid, nid],
            ).fetchone()
            if sc == 0 and not still:
                con.execute("DELETE FROM graph_nodes WHERE node_id = ?", [nid])
            else:
                con.execute(
                    """
                    UPDATE graph_nodes SET source_count = ?, confidence_avg = ?, metadata_json = ?, updated_at = ?
                    WHERE node_id = ?
                    """,
                    [sc, avg, json.dumps(md), now, nid],
                )
        con.commit()
    finally:
        con.close()


def _ensure_stub_node(
    con: duckdb.DuckDBPyConnection,
    normalized: str,
    document_id: str,
    chunk_id: str,
    now: datetime,
) -> None:
    """Create a minimal node when an edge references a name not present as an extracted entity."""
    nid = node_id_from_normalized(normalized)
    row = con.execute("SELECT 1 FROM graph_nodes WHERE node_id = ?", [nid]).fetchone()
    if row:
        return
    disp = normalized[:512]
    md = {
        "evidence": [
            {
                "chunk_id": chunk_id,
                "document_id": document_id,
                "confidence": 0.75,
            }
        ]
    }
    con.execute(
        """
        INSERT INTO graph_nodes (node_id, name, kind, normalized_name, source_count, confidence_avg,
          metadata_json, created_at, updated_at)
        VALUES (?, ?, 'concept', ?, 1, 0.75, ?, ?, ?)
        """,
        [nid, disp, normalized, json.dumps(md), now, now],
    )


def upsert_entities_edges(
    con: duckdb.DuckDBPyConnection,
    document_id: str,
    chunk_id: str,
    entities: list[ExtractedEntity],
    edges: list[ExtractedEdge],
) -> None:
    now = _utc_now()
    for ent in entities:
        nid = node_id_from_normalized(ent.normalized_name)
        row = con.execute(
            "SELECT metadata_json, source_count, confidence_avg, name, kind FROM graph_nodes WHERE node_id = ?",
            [nid],
        ).fetchone()
        md: dict[str, Any]
        if row:
            md = json.loads(row[0] or "{}")
            ev = list(md.get("evidence", []))
            replaced = False
            for i, e in enumerate(ev):
                if e.get("chunk_id") == chunk_id:
                    ev[i] = {
                        "chunk_id": chunk_id,
                        "document_id": document_id,
                        "confidence": ent.confidence,
                    }
                    replaced = True
                    break
            if not replaced:
                ev.append(
                    {
                        "chunk_id": chunk_id,
                        "document_id": document_id,
                        "confidence": ent.confidence,
                    }
                )
            md["evidence"] = ev
            confs = [float(e.get("confidence", 0.8)) for e in ev]
            sc = len(ev)
            avg = sum(confs) / max(sc, 1)
            con.execute(
                """
                UPDATE graph_nodes SET name = ?, kind = ?, normalized_name = ?, source_count = ?, confidence_avg = ?,
                  metadata_json = ?, updated_at = ?
                WHERE node_id = ?
                """,
                [
                    ent.display_name,
                    ent.kind,
                    ent.normalized_name,
                    sc,
                    avg,
                    json.dumps(md),
                    now,
                    nid,
                ],
            )
        else:
            md = {
                "evidence": [
                    {
                        "chunk_id": chunk_id,
                        "document_id": document_id,
                        "confidence": ent.confidence,
                    }
                ]
            }
            con.execute(
                """
                INSERT INTO graph_nodes (node_id, name, kind, normalized_name, source_count, confidence_avg,
                  metadata_json, created_at, updated_at)
                VALUES (?, ?, ?, ?, 1, ?, ?, ?, ?)
                """,
                [
                    nid,
                    ent.display_name,
                    ent.kind,
                    ent.normalized_name,
                    ent.confidence,
                    json.dumps(md),
                    now,
                    now,
                ],
            )

    for edge in edges:
        _ensure_stub_node(con, edge.source_normalized, document_id, chunk_id, now)
        _ensure_stub_node(con, edge.target_normalized, document_id, chunk_id, now)
        sid = node_id_from_normalized(edge.source_normalized)
        tid = node_id_from_normalized(edge.target_normalized)
        eid = edge_id_deterministic(
            document_id, chunk_id, edge.source_normalized, edge.target_normalized, edge.relation
        )
        con.execute(
            """
            INSERT INTO graph_edges (edge_id, source_node_id, target_node_id, relation, document_id, chunk_id, confidence, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT (edge_id) DO UPDATE SET
              confidence = excluded.confidence,
              created_at = excluded.created_at
            """,
            [eid, sid, tid, edge.relation, document_id, chunk_id, edge.confidence, now],
        )


def open_connection(duck_path: str) -> duckdb.DuckDBPyConnection:
    ensure_schema(duck_path)
    return duckdb.connect(duck_path)
