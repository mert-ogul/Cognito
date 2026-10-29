# Release checklist (revised MVP)

Use this for **manual GitHub Releases** and QA before publishing binaries.

## Pre-release

- [ ] Version bumped in `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml` (keep aligned).
- [ ] `CHANGELOG` or release notes drafted (user-visible fixes/features).
- [ ] `npm run test`, `cd sidecar && uv run pytest`, `cd src-tauri && cargo test` pass locally.
- [ ] `npm run build && npm run package:sidecar && cd src-tauri && cargo build --release` succeeds on the target OS (or rely on CI matrix).

## CI green

- [ ] Latest `main` passes `.github/workflows/ci.yml` (frontend, Rust, Python, tauri-smoke).

## Tag and build

- [ ] Create an annotated tag: `git tag -a v0.x.y -m "v0.x.y" && git push origin v0.x.y`
- [ ] Confirm `.github/workflows/release.yml` completes for **macOS ARM**, **macOS Intel**, and **Windows x64**.
- [ ] Download **Artifacts** from the workflow run (or attach outputs from local builds).

## Signing and notarization (optional, macOS)

> Do **not** commit certificates or passwords. Configure as **GitHub Actions secrets** or run locally in a trusted environment.

- [ ] Apple Developer Program membership and appropriate **Developer ID Application** certificate.
- [ ] Create `.p12` or use **Xcode** / `codesign` with identity stored in Keychain CI.
- [ ] **Notarization**: `notarytool` or `altool` with app-specific password / App Store Connect API key (team id, Apple ID).
- [ ] Staple the notarized app: `xcrun stapler staple ...`
- [ ] Placeholder env vars in workflow (see `release.yml` comments): `APPLE_*` secrets wired to real signing steps when ready.

## Windows signing (optional)

- [ ] **EV code signing** certificate (hardware token often required) or standard cert in protected CI.
- [ ] `signtool sign` with timestamp server; store cert as secret or use hosted signing.
- [ ] Placeholder env vars in workflow: `WINDOWS_*` — implement signing script when ready.

## Publishing

- [ ] Create **GitHub Release** from the tag; upload DMG/MSI/App/installer artifacts.
- [ ] Confirm **no secrets** appear in logs (mask secrets in Actions).

## Post-release

- [ ] Smoke-test install on a clean **macOS ARM**, **Intel**, and **Windows** VM from published assets.
- [ ] Verify sidecar starts (app health: sidecar reachable) and Ollama optional messaging matches expectations.

## Privacy

- [ ] Confirm the app makes no network requests except to the configured Ollama host.
