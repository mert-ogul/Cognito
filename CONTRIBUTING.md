# Contributing

## Setup

Install Node.js 20+, Rust (stable), and [uv](https://docs.astral.sh/uv/). From the repository root run `npm install`, then `cd sidecar && uv sync && cd ..`. Start the app with `npm run tauri dev`. Full chat and indexing need Ollama with the models configured in Settings.

## Checks before submitting

- `npm run test:unit`
- `npm run build`
- `cd sidecar && uv run pytest`
- `cd src-tauri && cargo test`
- Optional: `npm run test:e2e` after `npm run build:e2e`

## Rules

Keep pull requests small and focused on one change.

Match the naming and error-handling style of the surrounding code in each language.

Add or update tests when behaviour changes.

Leave no unused code, files, or dependencies after a change.

Do not upgrade dependencies unless the change requires it.

## Reporting issues

Use the GitHub issue templates when available. Include OS, Cognito version or commit, and steps to reproduce.
