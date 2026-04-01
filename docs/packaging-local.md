# Local packaging verification

## Prerequisites

- **Node** 20+ (CI uses 22)
- **Rust** stable + `cargo`
- **uv** ([astral.sh/uv](https://github.com/astral-sh/uv))
- OS-specific Tauri dependencies (see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/))

## One-shot release-style build

From the repository root (`cognito/`):

```bash
npm ci
npm run build
npm run package:sidecar
npx tauri build
```

- `package:sidecar` runs `uv sync` in `sidecar/` and copies the tree (including `.venv`) to `src-tauri/resources/sidecar/`.
- `tauri build` runs `beforeBuildCommand` again (repeats `npm run build` + `package:sidecar`); safe but slower.

## Cross-compile

Use explicit targets (Rust targets must be installed):

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc
npx tauri build --target aarch64-apple-darwin
```

On Linux hosts, macOS cross-compiles may require additional tooling; prefer **native runners** (CI matrix) for Apple binaries.

## Development vs packaged paths

| Mode | Sidecar root |
|------|----------------|
| `cargo tauri dev` | `../sidecar` next to `src-tauri` |
| Installed app | `resource_dir()/sidecar` (bundled copy) |
| Override | `COGNITO_SIDECAR_ROOT` or `COGNITO_PYTHON` |

See `src-tauri/src/sidecar_paths.rs`.

## Cleaning generated resources

```bash
rm -rf src-tauri/resources/sidecar/*
touch src-tauri/resources/sidecar/.gitkeep
```

(Or let git restore from `.gitignore` rules.)
