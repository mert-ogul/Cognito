# Contributing

## Principles

- Follow **[CONSTITUTION.md](./CONSTITUTION.md)** for product scope and engineering rules (file size limits, style).
- Prefer **small, focused PRs** with a clear motivation and test updates where behavior changes.
- **No drive-by refactors** unrelated to the task, match existing patterns in each language.

## Development setup

1. Install **Node.js 20+**, **Rust** (stable), and **uv** ([https://docs.astral.sh/uv/](https://docs.astral.sh/uv/)).
2. From the `cognito` directory: `npm install`, then `cd sidecar && uv sync && cd ..`.
3. Run the app: `npm run tauri dev` (Ollama with pulled models is required for full chat/embed/graph behavior).

## Checks before submitting

- `cargo test` in `src-tauri`
- `npm run test:unit` and `npm run test:py`
- `npm run build` (TypeScript + Vite)
- Optional: `npm run test:e2e` after `npm run build:e2e` (see Playwright config)

## Python

- Sidecar code lives under `sidecar/` with `uv` lockfile, run format/lint via `ruff` if configured locally.
- Use **`COGNITO_MOCK_EMBED=1`** in tests to avoid calling Ollama for embeddings (see `common/sqlite_vec_util.py`).

## Documentation

- Product contract: `CONSTITUTION.md`
- Architecture decisions: `docs/adr/`
- VitePress skeleton: `docs-site/` (`npm run docs:dev`)

## Reporting issues

Use GitHub issue templates when available (bug vs feature). Include OS, Cognito version or commit, and steps to reproduce.
