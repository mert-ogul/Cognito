# ADR 003: Bundled standalone CPython for the sidecar

## Status

Accepted

## Context

Release builds need a Python interpreter and dependencies on macOS ARM, macOS Intel, and Windows x64 without asking users to install Python. PyInstaller was rejected as heavier than needed.

## Decision

`scripts/package-sidecar.mjs` installs a standalone CPython 3.12 with uv, exports the sidecar requirements, installs those packages into that interpreter, and copies `main.py` plus the sidecar packages into `src-tauri/resources/sidecar/`. Development still uses `sidecar/.venv` from `uv sync`. Overrides are `COGNITO_SIDECAR_ROOT` and `COGNITO_PYTHON`.

## Consequences

Installer size includes the bundled interpreter and wheels. Dev and packaged layouts differ; path resolution lives in `src-tauri/src/sidecar_paths.rs`.
