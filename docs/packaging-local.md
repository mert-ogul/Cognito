# Local packaging verification

## Prerequisites

- Node 20+ (CI uses 22)
- Rust stable and cargo
- [uv](https://github.com/astral-sh/uv)
- OS-specific Tauri dependencies (see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/))

## One-shot release-style build

From the repository root:

```bash
npm ci
npm run build
npm run package:sidecar
npx tauri build
```

`package:sidecar` installs a standalone CPython, installs sidecar dependencies into it, and copies sources into `src-tauri/resources/sidecar/`. `tauri build` runs `beforeBuildCommand` again (repeats `npm run build` and `package:sidecar`); that is safe but slower.

## Cross-compile

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc
npx tauri build --target aarch64-apple-darwin
```

Prefer native runners for Apple binaries when possible.

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
