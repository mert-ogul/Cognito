"""Structured extraction records for DuckDB merge."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class ExtractedEntity:
    display_name: str
    kind: str
    normalized_name: str
    confidence: float


@dataclass(frozen=True)
class ExtractedEdge:
    source_normalized: str
    target_normalized: str
    relation: str
    confidence: float
