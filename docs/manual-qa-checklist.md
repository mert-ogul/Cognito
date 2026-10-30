# Manual QA checklist

Use this list before tagging a release candidate or public alpha.

## Environment

- [ ] First launch: the app opens; the toolbar status reflects sidecar and Ollama health.
- [ ] Missing Ollama: with Ollama stopped, the banner and chat stay blocked with a clear message.
- [ ] Missing model: a wrong or unpulled model name surfaces a readable error; Retry after pull works.

## Vault and import

- [ ] Choosing a folder indexes PDF and Markdown within the 100-document first-import cap.
- [ ] Import progress shows in Sources without freezing the shell.
- [ ] At 100 active documents, further import is refused with the documented message.
- [ ] Empty library: chat explains there is no indexed content (no invented citations).

## Index and graph

- [ ] After import, the graph pane loads nodes and edges for extracted content.
- [ ] Embedding failures mark documents failed; the rest of the app stays usable.
- [ ] Graph extraction failure does not corrupt the vault; chat still works with vector-only retrieval.

## Chat and citations

- [ ] Asking a question returns a grounded answer when Ollama and models are available.
- [ ] Answers include numbered citations when evidence is returned.
- [ ] Clicking a citation opens the passage pane with the source text.
- [ ] Selecting a graph node shows detail consistent with stored extraction.

## Library controls

- [ ] Switch folder: old index and graph are cleared; new folder indexes cleanly.
- [ ] Rebuild index: validates the folder before wiping; reindexes successfully.
- [ ] Remove library: Cognito forgets the folder; files remain on disk.

## Reliability

- [ ] With Ollama off after an index exists, missing query embeddings produce an explicit message.
- [ ] Restarting or killing the sidecar recovers without reinstalling; vault data stays intact.
- [ ] Removing a vault file is handled without crashing the shell.

## Packaged build

- [ ] Built app or installer launches, finds sidecar resources, completes one import and one chat turn.
