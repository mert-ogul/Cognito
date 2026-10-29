# Cognito

## What Cognito is

Cognito is a desktop app for asking questions about a folder of PDF and Markdown files. Indexing and chat run on your machine with a local Ollama install.

## What it does

- Indexes PDF and Markdown from one chosen folder (up to 100 documents on first import)
- Builds embeddings in SQLite and a background knowledge graph in DuckDB
- Answers with numbered citations tied to retrieved passages
- Shows a passage pane and a read-only graph view
- Lets you switch folders, rebuild the index, or remove the library without deleting your files

## Requirements

- Node.js 20 or newer, and npm
- Rust (stable via rustup)
- [uv](https://docs.astral.sh/uv/)
- [Ollama](https://ollama.com/) for chat and embeddings

## Setup

Pull the default models (or set others in Settings):

```bash
ollama pull phi4-mini
ollama pull nomic-embed-text
```

Install and run from the repository root:

```bash
npm install
cd sidecar && uv sync && cd ..
npm run tauri dev
```

Ollama should be reachable on the host the app uses (usually `127.0.0.1:11434`). Packaged builds ship a standalone CPython under `resources/sidecar/python`; development uses `sidecar/.venv`. Override with `COGNITO_PYTHON` or `COGNITO_SIDECAR_ROOT` if needed.

## Using it

Choose a folder from Sources or Settings. Cognito indexes PDF and Markdown files there. Ask questions in the chat column once the sidecar and chat model are ready. Citation markers open the passage pane; the graph pane shows extracted nodes and edges. Settings cover switching the folder, rebuilding the index, removing the library, and changing models. Sidecar logs live under the app data `logs` folder (on macOS: `~/Library/Application Support/com.mertogul.cognito/logs`). Use Open logs folder in Settings.

## Building an installer

```bash
npm run build
npm run package:sidecar
npm run tauri build
```

See [docs/packaging-local.md](docs/packaging-local.md) and [docs/release-checklist.md](docs/release-checklist.md).

## Tests

| Command | Purpose |
|--------|---------|
| `cargo test` | Rust (in `src-tauri`) |
| `npm run test:unit` | Vitest (frontend) |
| `npm run test:py` | Sidecar pytest |
| `npm run test:e2e` | Playwright (production build) |
| `npm run test` | Unit + Python |

## How it's put together

| Layer | Role |
|-------|------|
| Tauri / Rust | Window, vault paths, SQLite, import jobs, sidecar lifecycle |
| React | Shell UI, chat, sources, graph |
| Python sidecar | Parse, chunk, embed, graph store, retrieval prep |
| SQLite | Documents, chunks, embeddings |
| DuckDB | Graph nodes and edges |
| Ollama | Local chat and embedding API |

Decisions: [docs/adr/](docs/adr/).

## Privacy

Vault files stay on disk. Chat and embeddings go only to the Ollama host you configure. There is no telemetry. Details are in [ADR 004](docs/adr/0004-security-privacy.md).

## Licence

Apache License 2.0. See [LICENSE](./LICENSE).
