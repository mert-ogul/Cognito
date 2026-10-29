"""Graph snapshot and node_detail filter edges to returned nodes."""

from __future__ import annotations

from pathlib import Path

import duckdb

from common.duckdb_util import ensure_schema
from query.graph_read import node_detail, snapshot


def _seed(duck_path: str) -> None:
    ensure_schema(duck_path)
    con = duckdb.connect(duck_path)
    try:
        con.execute(
            """
            INSERT INTO graph_nodes
              (node_id, name, kind, normalized_name, source_count, confidence_avg,
               metadata_json, created_at, updated_at)
            VALUES
              ('n_a', 'A', 'concept', 'a', 3, 0.9, '{}', now(), now()),
              ('n_b', 'B', 'concept', 'b', 2, 0.8, '{}', now(), now()),
              ('n_c', 'C', 'concept', 'c', 1, 0.7, '{}', now(), now())
            """
        )
        con.execute(
            """
            INSERT INTO graph_edges
              (edge_id, source_node_id, target_node_id, relation, document_id, chunk_id,
               confidence, created_at)
            VALUES
              ('e_ab', 'n_a', 'n_b', 'relates', 'd1', 'c1', 0.9, now()),
              ('e_bc', 'n_b', 'n_c', 'relates', 'd1', 'c2', 0.8, now())
            """
        )
    finally:
        con.close()


def test_snapshot_limit_filters_edges(tmp_path: Path) -> None:
    duck = str(tmp_path / "g.duckdb")
    _seed(duck)
    out = snapshot(duck, 2)
    ids = {n["nodeId"] for n in out["nodes"]}
    assert ids == {"n_a", "n_b"}
    assert len(out["edges"]) == 1
    assert out["edges"][0]["edgeId"] == "e_ab"
    assert out["nodes"][0]["normalizedName"] == "a"
    assert "confidenceAvg" in out["nodes"][0]


def test_node_detail_known_and_unknown(tmp_path: Path) -> None:
    duck = str(tmp_path / "g.duckdb")
    _seed(duck)
    detail = node_detail(duck, "n_a")
    assert detail is not None
    assert detail["node"]["nodeId"] == "n_a"
    assert len(detail["edges"]) == 1
    assert detail["edges"][0]["edgeId"] == "e_ab"
    assert node_detail(duck, "missing") is None
