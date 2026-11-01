<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/branding/readme/cognito-readme-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="assets/branding/readme/cognito-readme-light.svg">
    <img alt="Cognito" src="assets/branding/readme/cognito-readme-light.svg" width="520">
  </picture>
</p>

## What Cognito is

Cognito is a desktop app for asking questions about PDF, Word and Markdown files in a folder you choose. Indexing and chat run on your machine with a local Ollama install.

<img src="docs/screenshots/chat-answer.png" width="100%" alt="Cognito answering a question about the Apollo 11 landing alarms, with the cited page open on the right.">

## What it does

- Indexes PDF, Word (`.docx`) and Markdown from one folder (up to 100 documents on first import)
- On macOS, recognises text in scanned PDF pages with Apple Vision; other platforms skip image-only pages
- Shows cropped page images under answer sources and card facts when PDF page regions were stored
- Opens the Passage pane on the full page with the cited region outlined
- Summarises each indexed document in the background into a checked document card (type, people, summary, facts)
- Optionally finds names, places and dates so documents can be linked
- Answers with numbered citations tied to retrieved passages
- Shows a Connections pane (Index and Timeline) for names and linked documents
- Lets you switch folders, rebuild the index, or remove the library without deleting your files

## Requirements

- Node.js 20 or newer, and npm
- Rust (stable via rustup)
- [uv](https://docs.astral.sh/uv/)
- [Ollama](https://ollama.com/) for chat and embeddings

## Setup

Pull the chat and embedding models (or set others in Settings):

```bash
ollama pull qwen3.5:9b
ollama pull qwen3-embedding:0.6b
```

Install and run from the repository root:

```bash
npm install
cd sidecar && uv sync && cd ..
npm run tauri dev
```

On macOS, also install the OCR extra so scanned PDFs can be read:

```bash
cd sidecar && uv sync --extra ocr && cd ..
```

Ollama should be reachable on the host the app uses (usually `127.0.0.1:11434`). Packaged builds ship a standalone CPython under `resources/sidecar/python`; development uses `sidecar/.venv`. Override with `COGNITO_PYTHON` or `COGNITO_SIDECAR_ROOT` if needed.

First launch downloads about 2.1 GB of checking and names models. You pick which Ollama models to use from Settings.

## Using it

Choose a folder from Library or Settings. Cognito indexes PDF, Word and Markdown files there (up to 100 on first import). Scanned PDFs on macOS are OCR'd and labelled in the library.

Ask questions in the chat column once the answer model is ready. Suggested questions appear when the chat is empty.

Sources under answers show the page crop when regions exist. Opening one shows the full page in the Passage pane with the cited region outlined.

Open a document for its card. Related documents appear under the card: shared names, mentions, or similar content. You can ask about both from there.

<img src="docs/screenshots/document-card.png" width="100%" alt="A document card for the Apollo 11 air-to-ground voice transcription, with people, summary and related documents.">

In Connections, Index lists people, organisations and other names from your files; Timeline shows dated documents and extracted dates.

<p>
  <img src="docs/screenshots/connections.png" width="49%" alt="Connections Index listing people and organisations linked across Apollo documents.">
  <img src="docs/screenshots/timeline.png" width="49%" alt="Connections Timeline of dated Apollo documents and extracted dates.">
</p>

Settings cover switching the folder, rebuilding the index, removing the library, answer / document-card / embedding models, summarise and find-names toggles, and Rebuild connections. Logs live under the app data `logs` folder (on macOS: `~/Library/Application Support/com.mertogul.cognito/logs`). Use Open logs folder in Settings.

Screenshots use public-domain NASA documents about the Apollo programme.

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
| Tauri / Rust | Window, library folder paths, SQLite, import jobs, sidecar lifecycle |
| React | Shell UI, chat, library, passage / connections |
| Python sidecar | Parse, chunk, embed, retrieval prep |
| SQLite | Documents, chunks, embeddings, cards, entities and links |
| Ollama | Local chat and embedding API |

Decisions: [docs/adr/](docs/adr/).

## Privacy

Your files stay on disk. Chat and embeddings go only to the Ollama host you configure. There is no telemetry. Details are in [ADR 004](docs/adr/0004-security-privacy.md).

## Licence

Apache License 2.0. See [LICENSE](./LICENSE).
