# Cognito

## What Cognito is

Cognito is a desktop app for asking questions about a folder of PDF, Word and Markdown files. Indexing and chat run on your machine with a local Ollama install.

## What it does

- Indexes PDF, Word (`.docx`) and Markdown from one chosen folder (up to 100 documents on first import)
- On macOS, recognises text in scanned PDF pages with Apple Vision; other platforms skip image-only pages
- Stores page regions for PDF chunks and shows cropped page images under answer sources and card facts
- Opens the Passage pane on the full page with the cited region outlined
- Summarises each indexed document in the background into a checked document card (type, people, summary, facts)
- Optionally finds names, places and dates so documents can be linked (connections in SQLite)
- Builds embeddings in SQLite
- Answers with numbered citations tied to retrieved passages
- Shows a Connections pane (Index, Map, Timeline) for names and linked documents
- Lets you switch folders, rebuild the index, or remove the library without deleting your files

## Requirements

- Node.js 20 or newer, and npm
- Rust (stable via rustup)
- [uv](https://docs.astral.sh/uv/)
- [Ollama](https://ollama.com/) for chat and embeddings

## Setup

Pull the default models (or set others in Settings):

```bash
ollama pull qwen3.5:9b
ollama pull qwen3.5:4b
ollama pull qwen3-embedding:0.6b
```

Install and run from the repository root:

```bash
npm install
cd sidecar && uv sync && cd ..
npm run tauri dev
```

Ollama should be reachable on the host the app uses (usually `127.0.0.1:11434`). Packaged builds ship a standalone CPython under `resources/sidecar/python`; development uses `sidecar/.venv`. Override with `COGNITO_PYTHON` or `COGNITO_SIDECAR_ROOT` if needed.

## Using it

Choose a folder from Library or Settings. Cognito indexes PDF, Word and Markdown files there, then summarises them in the background with the background model into document cards. Scanned PDFs on macOS are OCR’d and labelled in the library. With “Find names, places and dates” on (default), it also extracts names for connections. The Library is a scannable list (type, date, people); open a row for the full card and scroll its Pages section. Ask questions in the chat column once the sidecar and answer model are ready. Suggested questions appear when the chat is empty. Sources under answers show the page crop when regions exist; opening one shows the full page in the Passage pane. Settings cover switching the folder, rebuilding the index, removing the library, answer / background / embedding models, summarise and find-names toggles, and Rebuild connections. Sidecar logs live under the app data `logs` folder (on macOS: `~/Library/Application Support/com.mertogul.cognito/logs`). Use Open logs folder in Settings.

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
| React | Shell UI, chat, library, passage / connections |
| Python sidecar | Parse, chunk, embed, retrieval prep |
| SQLite | Documents, chunks, embeddings, cards, entities and links |
| Ollama | Local chat and embedding API |

Decisions: [docs/adr/](docs/adr/).

## Privacy

Vault files stay on disk. Chat and embeddings go only to the Ollama host you configure. There is no telemetry. Details are in [ADR 004](docs/adr/0004-security-privacy.md).

## Licence

Apache License 2.0. See [LICENSE](./LICENSE).
