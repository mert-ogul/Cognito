"""Create a work node for a document and link it to recurring entities."""

from __future__ import annotations

import json
from collections import Counter
from typing import Any

from extraction.graph_store import (
    edge_id_deterministic,
    node_id_from_normalized,
    open_connection,
    _utc_now,
)
from extraction.node_normalizer import normalize_name


def rpc_link_document(params: dict[str, Any]) -> dict[str, Any]:
    duck_path = str(params["duckPath"])
    document_id = str(params["documentId"])
    title = str(params.get("title") or "").strip() or document_id
    first_chunk_id = str(params.get("firstChunkId") or "").strip() or "link"
    norm_title = normalize_name(title)
    if len(norm_title) < 2:
        return {"ok": True, "linked": 0}

    con = open_connection(duck_path)
    try:
        now = _utc_now()
        work_id = node_id_from_normalized(norm_title)
        row = con.execute(
            "SELECT metadata_json, source_count FROM graph_nodes WHERE node_id = ?",
            [work_id],
        ).fetchone()
        evidence_entry = {
            "chunk_id": first_chunk_id,
            "document_id": document_id,
            "confidence": 1.0,
        }
        if row:
            md = json.loads(row[0] or "{}")
            ev = [e for e in md.get("evidence", []) if e.get("document_id") != document_id]
            ev.append(evidence_entry)
            md["evidence"] = ev
            con.execute(
                """
                UPDATE graph_nodes
                SET name = ?, kind = 'work', normalized_name = ?, source_count = ?,
                    confidence_avg = 1.0, metadata_json = ?, updated_at = ?
                WHERE node_id = ?
                """,
                [title[:512], norm_title, len(ev), json.dumps(md), now, work_id],
            )
        else:
            md = {"evidence": [evidence_entry]}
            con.execute(
                """
                INSERT INTO graph_nodes (
                  node_id, name, kind, normalized_name, source_count, confidence_avg,
                  metadata_json, created_at, updated_at
                ) VALUES (?, ?, 'work', ?, 1, 1.0, ?, ?, ?)
                """,
                [work_id, title[:512], norm_title, json.dumps(md), now, now],
            )

        counts: Counter[str] = Counter()
        name_by_norm: dict[str, str] = {}
        rows = con.execute(
            "SELECT name, kind, normalized_name, metadata_json FROM graph_nodes"
        ).fetchall()
        for name, kind, nname, mj in rows:
            if kind == "work" and nname == norm_title:
                continue
            md = json.loads(mj or "{}")
            chunks = {
                str(e.get("chunk_id"))
                for e in md.get("evidence", [])
                if e.get("document_id") == document_id and e.get("chunk_id")
            }
            if len(chunks) >= 2:
                counts[nname] += len(chunks)
                name_by_norm[nname] = str(name)

        linked = 0
        for nname, _freq in counts.most_common(25):
            tid = node_id_from_normalized(nname)
            eid = edge_id_deterministic(
                document_id, first_chunk_id, norm_title, nname, "discusses"
            )
            con.execute(
                """
                INSERT INTO graph_edges (
                  edge_id, source_node_id, target_node_id, relation,
                  document_id, chunk_id, confidence, created_at
                ) VALUES (?, ?, ?, 'discusses', ?, ?, 1.0, ?)
                ON CONFLICT (edge_id) DO UPDATE SET
                  confidence = excluded.confidence,
                  created_at = excluded.created_at
                """,
                [eid, work_id, tid, document_id, first_chunk_id, now],
            )
            linked += 1
        con.commit()
        return {"ok": True, "linked": linked, "workNodeId": work_id}
    finally:
        con.close()
