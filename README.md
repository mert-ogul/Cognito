# Cognito

**Constitution:** [CONSTITUTION.md](./CONSTITUTION.md) — authoritative rules for this repository.

## Product summary

Cognito is a **local-first desktop** workspace for **PDF and Markdown** vaults: semantic chunking, **sqlite-vec** embeddings, **LightRAG-style** graph extraction into **DuckDB**, and **deterministic hybrid retrieval** (vector-first, optional graph augmentation) with **Ollama** for chat and embeddings. The UI includes **read-only graph visualization** and **citation-aware** answers.

## Revised MVP scope

- **Formats:** PDF + Markdown only, **single vault** path.
- **Limits:** Up to **100** active documents on first import (sidecar batch size **20**).
- **Retrieval:** Fixed pipeline — no adaptive “router” model in MVP ([ADR 008](docs/adr/0008-deterministic-hybrid-not-classifier.md)).
- **Fallback:** If graph extraction or DuckDB reads fail, retrieval stays **vector-only** with explicit behavior, query embedding failure is surfaced when chunks exist but search cannot run ([sidecar `prepare_chat_retrieval`](sidecar/query/chat_pipeline.py)).
- **Defaults:** Chat `phi4-mini`, embeddings `nomic-embed-text` (configurable in settings).

## Intentionally out of scope (MVP)

Accounts, cloud sync, plugins, multi-vault management, graph editing, auto-updater UI, Microsoft GraphRAG-style cloud pipelines, and adaptive query classification. See `CONSTITUTION.md` for the full list.

## Prerequisites

- **Node.js** 20+ and **npm**
- **Rust** stable (`rustup`)
- **uv** — [https://docs.astral.sh/uv/](https://docs.astral.sh/uv/)
- **Ollama** locally for chat/embeddings in normal use

## Install (from source)

```bash
cd cognito
npm install
cd sidecar && uv sync && cd ..
```

## Ollama setup

1. Install [Ollama](https://ollama.com/) and start the daemon.
2. Pull the default models (or substitute your own in app settings):

```bash
ollama pull phi4-mini
ollama pull nomic-embed-text
```

3. Cognito expects Ollama on the default host/port used by the app (typically `127.0.0.1:11434`). If Ollama is stopped or a model is missing, chat warm-up and embedding steps **fail gracefully** with user-visible errors rather than silent success.

## Development

```bash
npm run tauri dev
```

The app may **block chat** until the sidecar reports readiness and the chat model is warmed, keep Ollama running during development.

**Sidecar:** Recreate the Python env with `cd sidecar && uv sync`. Override: `COGNITO_PYTHON=/path/to/python`. Packaged builds bundle `sidecar/.venv` next to `main.py` (see `src-tauri/src/sidecar_paths.rs`).

**VitePress docs (skeleton):**

```bash
npm install   # includes vitepress
npm run docs:dev
```

## Build

**Frontend:**

```bash
npm run build
```

**Tauri bundle** (`beforeBuildCommand` runs `npm run build` and `npm run package:sidecar`):

```bash
npm run tauri build
```

Details: [docs/packaging-local.md](docs/packaging-local.md), [docs/release-checklist.md](docs/release-checklist.md).

## Tests

| Command | Purpose |
|--------|---------|
| `cargo test` | Rust (in `src-tauri`) |
| `npm run test:unit` | Vitest (frontend) |
| `npm run test:py` | Sidecar pytest |
| `npm run test:e2e` | Playwright (uses production build, see `playwright.config`) |
| `npm run test` | Unit + Python |

E2E build variant: `npm run build:e2e` then run Playwright as configured.

## Benchmarks (import / index pipeline)

Measures **parse**, **chunk**, **SQLite insert**, **embed**, optional **graph extraction**, and **query** latency on a generated or `--corpus-dir` fixture set. Default uses **`COGNITO_MOCK_EMBED=1`** so embeddings do not require Ollama, use `--no-mock-embed` for end-to-end timing with Ollama.

```bash
npm run benchmark:import -- --count 50
# Optional: real embeddings
npm run benchmark:import -- --count 20 --no-mock-embed
# Graph phase (slow, needs Ollama + chat model)
npm run benchmark:import -- --count 5 --graph
```

Output is JSON including **chunk count**, per-phase **timings**, and **resident set** delta (platform-dependent, see script header). For a **50-document** run, use `--count 50` and compare `memoryMaxRssBytes` between start and end.

Script: [sidecar/scripts/benchmark_import.py](sidecar/scripts/benchmark_import.py).

## Architecture (overview)

| Layer | Role |
|-------|------|
| **Tauri / Rust** | Windowing, vault paths, SQLite migrations, import jobs, Ollama HTTP streaming, sidecar process lifecycle |
| **React** | Shell UI, chat, sources, graph canvas |
| **Python sidecar** | JSON-RPC: parse, chunk, embed, graph extract, hybrid retrieval, graph snapshot |
| **SQLite** | Documents, chunks, sqlite-vec embeddings |
| **DuckDB** | Graph nodes/edges (adjacency) |
| **Ollama** | Local LLM + embedding API |

Further detail: [docs-site/guide/architecture.md](docs-site/guide/architecture.md) (skeleton), [docs/adr/](docs/adr/).

## Privacy

- **Local content:** Vault files stay on disk, indexing reads them locally.
- **Inference:** Chat and embeddings go to **your** Ollama instance unless you change deployment.
- **Telemetry:** None by default, opt-in only if ever added ([ADR 004](docs/adr/0004-security-privacy-boundaries.md)).

## ADRs (decisions)

| ADR | Topic |
|-----|--------|
| [0001](docs/adr/0001-hybrid-retrieval.md) | Deterministic hybrid retrieval |
| [0002 stack](docs/adr/0002-revised-mvp-stack.md) | LightRAG, DuckDB, uv |
| [0002 architecture](docs/adr/0002-revised-mvp-architecture.md) | Process boundaries |
| [0003](docs/adr/0003-sidecar-bundling-with-uv.md) | uv sidecar bundling |
| [0004](docs/adr/0004-security-privacy-boundaries.md) | Security & privacy |
| [0005](docs/adr/0005-constitution-overrides-prior-plan.md) | Constitution vs older plans |
| [0006](docs/adr/0006-lightrag-style-extraction.md) | LightRAG-style extraction |
| [0007](docs/adr/0007-duckdb-adjacency-graph.md) | DuckDB adjacency |
| [0008](docs/adr/0008-deterministic-hybrid-not-classifier.md) | No adaptive classifier in MVP |

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

## Manual QA

Release checklist: [docs/manual-qa-checklist.md](docs/manual-qa-checklist.md).

## License

Apache License 2.0 — see [LICENSE](./LICENSE).
