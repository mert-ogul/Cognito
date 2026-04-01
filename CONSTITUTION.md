# Cognito repository constitution

This document is the **authoritative** product and engineering contract for Cognito. The **revised build plan** overrides any older plan when they conflict.

## Product

Cognito is a **local-first desktop AI knowledge workspace**. All **document content remains local**. No silent analytics or crash reporting, any telemetry must be **opt-in** and **disabled by default**.

## Authoritative build rules

- **Graph extraction**: LightRAG.
- **Graph store**: DuckDB adjacency tables.
- **Python**: uv + bundled venv sidecar.
- **Query**: deterministic **vector-first** retrieval with **graph augmentation** — not an adaptive classifier MVP.
- **Scope**: PDF + Markdown only, **single vault**, **read-only graph**, **max 100 documents** on first import, **no** accounts, sync, plugins, graph editing, community summaries, global synthesis mode, or multi-vault management.
- **Offline-first**, **local-first**, **no cloud dependence** for core functionality.
- **Ollama**: pre-warm chat model on app launch, block querying until model is ready where the product specifies it.
- **Fallback**: if graph is unavailable, retrieval **falls back to vector-only**.
- **Default models**: `phi4-mini` (chat), `nomic-embed-text` (embeddings), unless explicitly configured otherwise.

## Stack (target)

- Tauri 2.0, Rust backend
- React 18 + TypeScript + Vite
- shadcn/ui + Tailwind
- Zustand + TanStack Query
- Cytoscape.js (read-only graph)
- Python sidecar (uv)
- Ollama (local)
- SQLite + sqlite-vec (see ADRs for current storage details)
- DuckDB for graph adjacency (sidecar-owned in current architecture)
- GitHub Actions CI
- **License**: Apache 2.0
- **Docs**: VitePress (documentation site is part of the intended stack)

## Code quality standards

### File / module

- Max file length: **500 lines**
- Max top-level **public** types per file: **1**
- Exception: small related helpers, enums, interfaces, type aliases

### Class

- Max class length: **250 lines**
- Max public methods per class: **10**
- Max total methods per class: **20**
- Max instance fields: **8**
- Max constructor parameters: **5**
- Max injected dependencies: **5**
- Max inheritance depth: **3**
- Max responsibilities per class: **1**

### Function / method

- Max function length: **25 logical lines**
- Preferred function length: **5–15 lines**
- Max parameters: **4**
- Max boolean parameters: **1**
- Max nesting depth: **3**
- Max cyclomatic complexity: **10**
- Max return statements: **3**
- Max local variables in active use: **7**

### Comments

- Target comment density: **5–15%**
- Hard limit: **20%**
- Comment public APIs only when not obvious
- Inline comments in function body: **0–2** preferred
- No block comments in functions unless unavoidable
- TODO/FIXME must include **owner or ticket reference**

### Conditionals / control flow

- Max if/else-if chain length: **3**
- Max loop body length: **20 lines**
- Max try/catch body length: **15 lines**
- Max catch branches: **3**

### Duplication

- No duplicated business rule in more than **1** place
- No copy-pasted block over **8 lines**
- Extract shared patterns at **3** occurrences unless it hurts clarity

### Naming

- Type names: **2–4 words** max and role-revealing
- Method names: **action + object**
- Variable names: usually **1–3 words**
- Avoid vague names like Manager, Helper, Util, Data, Info, Handler, Processor unless domain-accurate

## Execution protocol (every task)

1. Briefly restate the goal in implementation terms.
2. List exact files you will touch.
3. Implement production-ready code, not pseudocode.
4. Run tests/build/lint for touched areas.
5. Fix failures before stopping.
6. End with: **completed**, **tests run**, **remaining gaps**, **next recommended prompt**.

## Guardrails

- Never create files longer than **500 lines**, prefer more small files over fewer large files.
- One top-level public type per file unless tiny helper types are tightly related.
- Do not introduce vague names (Manager, Helper, Util, Handler, Processor, etc.) unless domain-accurate.
- Do not add speculative abstractions.
- Do not create dead code or fake adapters for future features.
- No TODO without owner or ticket.
- No placeholder code unless explicitly marked and isolated.
- Preserve **offline-first** and **privacy-by-default** behavior.
- Do not silently add analytics or crash reporting.

## Conflict resolution

When documents disagree, follow the **revised build plan** first, then this constitution, then ADRs, then the rest of the repo.
