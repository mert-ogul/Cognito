# Release checklist

Use this for manual GitHub Releases and QA before publishing binaries.

## Pre-release

- [ ] Version bumped in `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml` (keep aligned).
- [ ] Changelog or release notes drafted (user-visible fixes and features).
- [ ] `npm run test`, `cd sidecar && uv run pytest`, and `cd src-tauri && cargo test` pass locally.
- [ ] `npm run build && npm run package:sidecar && cd src-tauri && cargo build --release` succeeds on the target OS (or rely on the CI matrix).

## CI green

- [ ] Latest `main` passes `.github/workflows/ci.yml` (frontend, Rust, Python, tauri-smoke).

## Tag and build

- [ ] Create an annotated tag: `git tag -a v0.x.y -m "v0.x.y" && git push origin v0.x.y`
- [ ] Confirm `.github/workflows/release.yml` completes for macOS ARM, macOS Intel, and Windows x64.
- [ ] Download artifacts from the workflow run (or attach outputs from local builds).

## Signing and notarization (optional, macOS)

Do not commit certificates or passwords. Configure GitHub Actions secrets or run locally in a trusted environment.

- [ ] Apple Developer Program membership and a Developer ID Application certificate.
- [ ] Create a `.p12` or use Xcode / `codesign` with the identity in Keychain CI.
- [ ] Notarization with `notarytool` or `altool` (team id, Apple ID or App Store Connect API key).
- [ ] Staple the notarized app: `xcrun stapler staple ...`
- [ ] Placeholder env vars in the workflow (`APPLE_*`) wired when signing is ready.

## Windows signing (optional)

- [ ] EV or standard code signing certificate available to CI.
- [ ] `signtool sign` with a timestamp server.
- [ ] Placeholder env vars in the workflow (`WINDOWS_*`) wired when signing is ready.

## Publishing

- [ ] Create a GitHub Release from the tag; upload DMG/MSI/App/installer artifacts.
- [ ] Confirm no secrets appear in logs.

## Post-release

- [ ] Smoke-test install on clean macOS ARM, Intel, and Windows from published assets.
- [ ] Verify the sidecar starts and Ollama messaging matches expectations.

## Privacy

- [ ] Confirm the app makes no network requests except to the configured Ollama host.
