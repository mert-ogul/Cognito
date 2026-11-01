# Manual QA checklist

Run through this before tagging a release candidate or a public alpha. Tick items on a real machine with Ollama installed; automated CI does not cover these paths.

## Environment

- [ ] First launch: the app opens; the toolbar status reflects health. Checking and names models download on first launch (~2.1 GB).
- [ ] Missing Ollama: with Ollama stopped, the banner and chat stay blocked with a clear message.
- [ ] Missing model: a wrong or unpulled model name surfaces a readable error; Retry after pull works.

## Library and import

- [ ] Choosing a folder indexes PDF and Markdown within the 100-document first-import cap.
- [ ] Import progress shows in Sources without freezing the shell.
- [ ] At 100 active documents, further import is refused with the documented message.
- [ ] Empty library: chat explains there is no indexed content (no invented citations).

## Index and connections

- [ ] After import, document cards fill in and Connections Index lists extracted names when find-names is on.
- [ ] Embedding failures mark documents failed; the rest of the app stays usable.
- [ ] Entity extraction failure does not corrupt the library; chat still works.

## Chat and citations

- [ ] Asking a question returns a grounded answer when Ollama and models are available.
- [ ] Answers include numbered citations when evidence is returned.
- [ ] Clicking a citation opens the passage pane with the source text and page image when regions exist.
- [ ] Related documents on a document page open the other document; Ask about both scopes chat to both.

## Library controls

- [ ] Switch folder: old index is cleared; new folder indexes cleanly.
- [ ] Rebuild index: validates the folder before wiping; reindexes successfully.
- [ ] Remove library: Cognito forgets the folder; files remain on disk.

## Reliability

- [ ] With Ollama off after an index exists, missing query embeddings produce an explicit message.
- [ ] Restarting the app recovers without reinstalling; library data stays intact.
- [ ] Removing a library file is handled without crashing the shell.

## Packaged build

- [ ] Built app or installer launches, finds bundled resources, completes one import and one chat turn.
