# Manual QA checklist (release candidate)

Use this list before tagging a release candidate or public alpha. Adjust expected behavior to the current MVP in [CONSTITUTION.md](../CONSTITUTION.md).

## Environment

- [ ] **First launch**: App opens; status area shows sidecar and subsystem health when applicable.
- [ ] **Missing Ollama**: With Ollama stopped, UI shows a clear blocked/warm-up state; no silent crash when opening chat or settings.
- [ ] **Missing model**: Wrong or unpulled model name surfaces a readable error (pull / fix model in settings).

## Vault and import

- [ ] **Import folder**: PDF/Markdown under the vault import successfully within the 100-document first-import cap.
- [ ] **Progress display**: Import job shows progress phases (parse/index) without freezing the shell.
- [ ] **Over-cap first import**: At 100 active documents, additional import is refused with the documented message.
- [ ] **Empty vault**: Chat explains no indexed content when the library is empty (not hallucinated citations).

## Index and graph

- [ ] **Graph builds**: After import, graph snapshot loads; nodes/edges appear for extracted content.
- [ ] **Embedding failure path**: If embeddings cannot be produced, documents fail with a recorded error; app remains usable.
- [ ] **Graph extraction failure**: Failure does not corrupt the vault; vector search still works where embeddings exist (vector-only fallback).

## Chat and citations

- [ ] **Ask question**: Grounded answer path runs with Ollama when models are available.
- [ ] **Receive citations**: Assistant responses include chunk citations when evidence is returned.
- [ ] **Open citation preview**: Clicking a citation opens preview and highlights relevant text where implemented.
- [ ] **Node detail**: Graph node selection shows detail consistent with stored extraction.

## Reliability

- [ ] **Offline usage**: With Ollama off after index exists, behavior degrades gracefully (no indexed query embeddings → explicit message, not invented sources).
- [ ] **Recovery after sidecar restart**: Restarting the sidecar (or killing Python child) recovers without requiring a full app reinstall; vault data intact.
- [ ] **Deleted files**: Removing a vault file is handled per product rules (re-index or stale handling); no crash.

## Packaged build

- [ ] **Packaged app smoke test**: Built `.app` / installer launches, finds sidecar resources, completes one import and one chat turn.
