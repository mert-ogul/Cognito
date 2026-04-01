# ADR 003: Sidecar bundling with uv (no PyInstaller)

## Status

Accepted

## Context

The constitution requires **uv + bundled venv**, explicitly **not** PyInstaller. Release builds must run on **macOS ARM**, **macOS Intel**, and **Windows x64** with a **stable** sidecar layout.

## Decision

1. **Development**: Sidecar code lives in `sidecar/`; developers run `uv sync` and execute Python from `sidecar/.venv`. Rust resolves paths relative to `CARGO_MANIFEST_DIR/../sidecar` when no bundled resource is present.
2. **Production**: Before `cargo tauri build`, **`npm run package:sidecar`** runs `uv sync` and copies `sidecar/` (including `.venv`) into **`src-tauri/resources/sidecar/`**. Tauri `bundle.resources` includes that folder in the app bundle.
3. **Runtime resolution** (see `src-tauri/src/sidecar_paths.rs`): Prefer `resource_dir()/sidecar` when `main.py` exists; else fall back to development paths. Override via **`COGNITO_SIDECAR_ROOT`** or **`COGNITO_PYTHON`** for debugging.
4. **Git** ignores generated files under `resources/sidecar/**` except `.gitkeep` so the directory exists for `cargo check` without committing the venv.

## Consequences

- CI and release jobs **must** run `package:sidecar` (or equivalent) before bundling.
- Bundle size includes the full `.venv`; acceptable for MVP; future work may trim optional wheels per platform.
- **PyInstaller** is not used and should not be introduced without a new ADR.

## Related

- `docs/packaging-local.md` — local verification steps.
- `scripts/package-sidecar.mjs` — implementation.
