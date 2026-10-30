"""GLiNER label sets by document card type."""

from __future__ import annotations

BASE: list[str] = [
    "person",
    "organization",
    "place",
    "date",
    "amount of money",
]

BY_TYPE: dict[str, list[str]] = {
    "paper": [
        "method",
        "model",
        "dataset",
        "metric",
        "research field",
    ],
    "contract": ["obligation", "deadline", "product or service"],
    "agreement": ["obligation", "deadline", "product or service"],
    "letter": ["school", "program", "job title", "deadline"],
    "email": ["school", "program", "job title", "deadline"],
    "cv": ["school", "program", "job title", "deadline"],
    "form": ["school", "program", "job title", "deadline"],
    "invoice": ["product or service"],
    "receipt": ["product or service"],
    "report": ["topic", "product"],
    "manual": ["topic", "product"],
    "notes": ["topic", "product"],
    "other": ["topic", "product"],
    "article": ["topic", "product"],
    "presentation": ["topic", "product"],
    "book": ["topic", "product"],
}


def labels_for_document(card_type: str | None, extra: list[str]) -> list[str]:
    """BASE + type-specific labels + extras, de-duplicated, order preserved."""
    seen: set[str] = set()
    out: list[str] = []
    typed = BY_TYPE.get((card_type or "").strip().lower(), [])
    for label in [*BASE, *typed, *(extra or [])]:
        s = str(label or "").strip()
        if not s:
            continue
        key = s.lower()
        if key in seen:
            continue
        seen.add(key)
        out.append(s)
    return out
