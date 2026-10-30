# ADR 003: Bundled standalone CPython for the sidecar

## Status

Accepted

## Context

Release builds need a Python interpreter and dependencies on macOS ARM, macOS Intel, and Windows x64 without asking users to install Python. PyInstaller was rejected as heavier than needed. Page rendering, Word import and (on macOS) OCR add packages that must ship in the same bundle.

## Decision

`scripts/package-sidecar.mjs` installs a standalone CPython 3.12 with uv, exports the sidecar requirements, installs those packages into that interpreter, and copies `main.py` plus the sidecar packages into `src-tauri/resources/sidecar/`. Development still uses `sidecar/.venv` from `uv sync`. Overrides are `COGNITO_SIDECAR_ROOT` and `COGNITO_PYTHON`.

Alongside indexing and query libraries, the sidecar also packages:

- `python-docx` for `.docx` import
- `ocrmac` for Apple Vision OCR on scanned PDF pages (macOS only; imported only when needed)
- `pymupdf` for PDF text, block boxes and page/region PNG rendering

Page image caches live under the app data directory (`cache/pages`), not inside the sidecar tree.

## Consequences

Installer size includes the bundled interpreter and wheels, including OCR and Word support. Dev and packaged layouts differ; path resolution lives in `src-tauri/src/sidecar_paths.rs`. Windows and Linux builds skip OCR and mark image-only pages as having no text.
