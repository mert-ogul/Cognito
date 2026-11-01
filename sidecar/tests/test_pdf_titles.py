"""PDF title junk checks and candidate order."""

from __future__ import annotations

from pathlib import Path
from unittest import mock

import fitz

from ingestion.parsing import (
    _filename_reads_like_name,
    _title_is_junk,
    parse_file,
    pdf_title,
)


def test_junk_rejects_letter_spaced_and_repeated_char() -> None:
    assert _title_is_junk("p R E s s K")
    assert _title_is_junk("i!iJiiiii+iiiiiii+iii+i")
    assert _title_is_junk("... ... ...")
    assert not _title_is_junk("Apollo 12 Mission Report")


def test_filename_reads_like_name() -> None:
    assert _filename_reads_like_name("Apollo_12_Mission_Report")
    assert not _filename_reads_like_name("A12_PressKit")
    assert not _filename_reads_like_name("1710.10903v3")


def test_metadata_title_wins_when_clean(tmp_path: Path) -> None:
    pdf = tmp_path / "file.pdf"
    doc = fitz.open()
    page = doc.new_page()
    page.insert_text((40, 80), "Body text here about the lunar module.", fontsize=11)
    doc.set_metadata({"title": "Apollo 11 Preliminary Science Report"})
    doc.save(pdf)
    doc.close()
    assert parse_file(pdf)["title"] == "Apollo 11 Preliminary Science Report"


def test_filename_before_page_title_when_name_like(tmp_path: Path) -> None:
    pdf = tmp_path / "Apollo_12_Mission_Report.pdf"
    doc = fitz.open()
    page = doc.new_page()
    page.insert_text((40, 60), "JUNK TITLE LINE HERE", fontsize=28)
    page.insert_text((40, 120), "Body of the mission report continues.", fontsize=11)
    doc.set_metadata({"title": "p R E s s K"})
    doc.save(pdf)
    doc.close()
    with mock.patch(
        "ingestion.parsing._largest_font_title",
        return_value="JUNK TITLE LINE HERE",
    ):
        # Filename reads like a name → used before page font title.
        assert parse_file(pdf)["title"] == "Apollo 12 Mission Report"


def test_page_font_title_when_filename_is_code(tmp_path: Path) -> None:
    pdf = tmp_path / "A12_PressKit.pdf"
    doc = fitz.open()
    page = doc.new_page()
    page.insert_text((40, 60), "Apollo 12 Press Kit", fontsize=28)
    page.insert_text((40, 120), "Press briefing materials for the flight.", fontsize=11)
    doc.set_metadata({"title": ""})
    doc.save(pdf)
    doc.close()
    title = parse_file(pdf)["title"]
    assert title == "Apollo 12 Press Kit"


def test_spaced_filename_last_fallback(tmp_path: Path) -> None:
    pdf = tmp_path / "A12_PressKit.pdf"
    doc = fitz.open()
    page = doc.new_page()
    page.insert_text((40, 80), ".... .... ....", fontsize=20)
    doc.set_metadata({"title": "i!iJiiiii+iiiiiii"})
    doc.save(pdf)
    doc.close()
    with mock.patch("ingestion.parsing._largest_font_title", return_value="i!iJiiiii"):
        assert pdf_title(fitz.open(pdf), pdf) == "A12 PressKit"
