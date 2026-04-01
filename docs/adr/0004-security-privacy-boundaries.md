# ADR 004: Security and privacy boundaries (revised MVP)

## Status

Accepted

## Context

Cognito processes **sensitive local documents**. The product must minimize attack surface, avoid silent exfiltration, and keep **telemetry and crash reporting opt-in** and **off by default** (see constitution).

## Decision

### Tauri shell

1. **Command allowlist**: Only registered `tauri::command` handlers are callable from the frontend; there is no generic `eval` or arbitrary file read from the UI layer.
2. **Capabilities**: `src-tauri/capabilities/default.json` grants **minimal** permissions (`core:default`, `core:event:default`, `dialog:default`). New permissions require review.
3. **File access**: Document content and previews flow through Rust commands and SQLite/DuckDB paths configured by the app — not raw user-supplied filesystem URLs from the webview without validation.

### Sidecar

4. **IPC**: The sidecar communicates with Rust via **stdin/stdout** JSON-RPC only — no advertised network port for core document processing.
5. **Network**: Ingestion and retrieval may call **Ollama** (and similar **user-local** endpoints) as designed; **document bytes and indexed text are not sent to external SaaS** by Cognito core. Any future opt-in telemetry must not include document content.

### Privacy defaults

6. **Telemetry** and **crash reporting** are stored as settings flags defaulting to **false**; no background analytics without explicit opt-in.
7. **No in-app auto-updater** in this MVP scope (reduces supply-chain and consent complexity).

### Distribution

8. **Signing / notarization** (Apple) and **Authenticode** (Windows) are **release-operator** steps; CI jobs document env var placeholders without embedding secrets.

## Consequences

- Third-party dependencies (Rust, Python, Node) must be kept updated on a maintenance schedule.
- A future **auto-updater** or **cloud sync** would require a separate ADR and explicit user consent.
