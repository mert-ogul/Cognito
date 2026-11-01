# Bundled resources

The `sidecar/` subtree is filled by `npm run package:sidecar` before release builds (it copies `../sidecar`, including the uv-managed `.venv`). A `.gitkeep` keeps the path present so `cargo check` and Tauri config validation succeed without a full package step.

Do not commit the full packaged venv; CI and release jobs run `package:sidecar` before `cargo tauri build`.
