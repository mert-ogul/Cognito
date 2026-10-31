"""Normalize entity names, dates, and search text (ligatures / hyphens)."""

from __future__ import annotations

import re
import unicodedata

_TRAILING_PUNCT = re.compile(r"[\s\"'`.,;:!?)\]]+$")
_LEADING_PUNCT = re.compile(r"^[\[(\"'`]+")
_WS_RE = re.compile(r"\s+")
_SOFT_HYPHEN = "\u00ad"
_LINE_END_HYPHEN_RE = re.compile(r"(\w)-\s*\n\s*(\w)")

_SPACING_ACCENTS = {
    chr(a): chr(b)
    for a, b in (
        (0x00B4, 0x0301),
        (0x0060, 0x0300),
        (0x00A8, 0x0308),
        (0x02C7, 0x030C),
        (0x02C6, 0x0302),
        (0x02DC, 0x0303),
        (0x02D8, 0x0306),
        (0x02D9, 0x0307),
        (0x02DA, 0x030A),
        (0x02DD, 0x030B),
    )
}
_ACCENT_BEFORE_LETTER_RE = re.compile(
    "([" + "".join(_SPACING_ACCENTS) + r"])([^\W\d_])"
)


def fix_pdf_accents(text: str) -> str:
    return _ACCENT_BEFORE_LETTER_RE.sub(
        lambda m: m.group(2) + _SPACING_ACCENTS[m.group(1)], text
    )


_MONTHS = {
    "january": 1,
    "jan": 1,
    "february": 2,
    "feb": 2,
    "march": 3,
    "mar": 3,
    "april": 4,
    "apr": 4,
    "may": 5,
    "june": 6,
    "jun": 6,
    "july": 7,
    "jul": 7,
    "august": 8,
    "aug": 8,
    "september": 9,
    "sep": 9,
    "sept": 9,
    "october": 10,
    "oct": 10,
    "november": 11,
    "nov": 11,
    "december": 12,
    "dec": 12,
}

_ISO_DATE = re.compile(r"^(\d{4})(?:-(\d{1,2})(?:-(\d{1,2}))?)?$")
_SLASH_DATE = re.compile(r"^(\d{1,2})[/-](\d{1,2})[/-](\d{4})$")
_MONTH_DAY_YEAR = re.compile(
    r"^(?P<month>[A-Za-z]+)\s+(?P<day>\d{1,2})(?:st|nd|rd|th)?,?\s+(?P<year>\d{4})$",
    re.IGNORECASE,
)
_MONTH_YEAR = re.compile(
    r"^(?P<month>[A-Za-z]+)\s+(?P<year>\d{4})$",
    re.IGNORECASE,
)
_DAY_MONTH_YEAR = re.compile(
    r"^(?P<day>\d{1,2})(?:st|nd|rd|th)?\s+(?P<month>[A-Za-z]+)\s+(?P<year>\d{4})$",
    re.IGNORECASE,
)
_YEAR_ONLY = re.compile(r"^(\d{4})$")


def normalize_name(name: str) -> str:
    s = unicodedata.normalize("NFKC", name or "")
    s = s.strip()
    s = _LEADING_PUNCT.sub("", s)
    s = _TRAILING_PUNCT.sub("", s)
    s = s.strip()
    s = _WS_RE.sub(" ", s)
    lower = s.lower()
    if lower.startswith("the "):
        s = s[4:].strip()
        lower = s.lower()
    return lower


def _pad2(n: int) -> str:
    return f"{n:02d}"


def normalize_date(text: str) -> str | None:
    """Return YYYY-MM-DD, YYYY-MM, or YYYY when parseable; else None."""
    raw = _WS_RE.sub(" ", (text or "").strip())
    if not raw:
        return None

    m = _ISO_DATE.match(raw)
    if m:
        y, mo, d = m.group(1), m.group(2), m.group(3)
        if mo is None:
            return y
        mi = int(mo)
        if mi < 1 or mi > 12:
            return None
        if d is None:
            return f"{y}-{_pad2(mi)}"
        di = int(d)
        if di < 1 or di > 31:
            return None
        return f"{y}-{_pad2(mi)}-{_pad2(di)}"

    m = _YEAR_ONLY.match(raw)
    if m:
        return m.group(1)

    m = _SLASH_DATE.match(raw)
    if m:
        a, b, y = int(m.group(1)), int(m.group(2)), m.group(3)
        if a > 12 and b <= 12:
            day, month = a, b
        elif b > 12 and a <= 12:
            month, day = a, b
        else:
            month, day = a, b
        if month < 1 or month > 12 or day < 1 or day > 31:
            return None
        return f"{y}-{_pad2(month)}-{_pad2(day)}"

    m = _MONTH_DAY_YEAR.match(raw)
    if m:
        month = _MONTHS.get(m.group("month").lower())
        day = int(m.group("day"))
        year = m.group("year")
        if month is None or day < 1 or day > 31:
            return None
        return f"{year}-{_pad2(month)}-{_pad2(day)}"

    m = _DAY_MONTH_YEAR.match(raw)
    if m:
        month = _MONTHS.get(m.group("month").lower())
        day = int(m.group("day"))
        year = m.group("year")
        if month is None or day < 1 or day > 31:
            return None
        return f"{year}-{_pad2(month)}-{_pad2(day)}"

    m = _MONTH_YEAR.match(raw)
    if m:
        month = _MONTHS.get(m.group("month").lower())
        year = m.group("year")
        if month is None:
            return None
        return f"{year}-{_pad2(month)}"

    return None


def normalize_search_text(text: str) -> str:
    """NFKC fold ligatures, drop soft hyphens, join hyphenated line breaks."""
    s = unicodedata.normalize("NFKC", text or "")
    s = s.replace(_SOFT_HYPHEN, "")
    s = _LINE_END_HYPHEN_RE.sub(r"\1\2", s)
    return s


def normalize_pdf_text(text: str) -> str:
    """PDF spacing accents, then the shared search-text normalisation."""
    return normalize_search_text(fix_pdf_accents(text or ""))
