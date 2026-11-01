"""Chunk role detection, labels, and name normalizer."""

from __future__ import annotations

from common.text_norm import normalize_name
from ingestion.chunking import (
    assign_chunk_role,
    citation_line_ratio,
    extract_chunk_labels,
    is_references_heading,
)


def test_references_heading_small_caps() -> None:
    assert is_references_heading("R EFERENCES")
    assert is_references_heading("# References")
    assert is_references_heading("Bibliography")
    assert is_references_heading("ACKNOWLEDGEMENTS")
    assert not is_references_heading("Related work")


def test_citation_lines_mark_references_role() -> None:
    refs = "\n".join(
        [
            "13. S. Milgram, Psychol. Today 2, 60 (1967).",
            "14. A. Author, Nature 1, 2 (2001).",
            "15. B. Writer, Science 3, 4 (1999).",
            "Some leftover note without a year here.",
        ]
    )
    assert citation_line_ratio(refs) >= 0.4
    assert assign_chunk_role(refs, [], after_references_heading=False) == "references"


def test_ordinary_paragraph_with_year_stays_body() -> None:
    body = (
        "In 2017, graph attention networks were proposed as an alternative "
        "to spectral methods. The approach operates on graph-structured data."
    )
    assert citation_line_ratio(body) < 0.4
    assert assign_chunk_role(body, [], after_references_heading=False) == "body"


def test_after_references_heading_forces_role() -> None:
    assert assign_chunk_role("anything", [], after_references_heading=True) == "references"


def test_extract_chunk_labels_equations_and_captions() -> None:
    text = "\n".join(
        [
            "Some body text.",
            "(2)",
            "More text.",
            "Table 3. Accuracy on Cora",
            "Figure 1 shows the architecture.",
            "Fig. 2: Ablation",
            "(2)",
        ]
    )
    assert extract_chunk_labels(text) == ["(2)", "Table 3", "Figure 1", "Fig. 2"]


def test_normalize_strips_quotes_punct_and_the() -> None:
    assert normalize_name('  "Graph Attention Networks". ') == "graph attention networks"
    assert normalize_name("The GCN model") == "gcn model"
    assert normalize_name("(Ada)") == "ada"
