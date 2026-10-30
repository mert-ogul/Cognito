export const E2E_HEALTH = {
  sqliteOk: true,
  sidecarReachable: true,
  ollamaModelReady: true,
  vaultConfigured: true,
  chatModel: "qwen3.5:9b",
  embedModel: "qwen3-embedding:0.6b",
  chatModelWarmed: true,
  warmUpPending: false,
  warmUpError: null,
  ollamaReachable: true,
  missingModels: [] as string[],
  indexFormatUpdating: false,
  checkingModelsReady: true,
} as const;

export const E2E_DOCUMENTS = [
  {
    id: "e2e-doc-1",
    path: "/fixture/sample.md",
    title: "Fixture sample",
    fileType: "markdown",
    contentHash: "e2e",
    pageCount: null,
    status: "indexed",
    errorMessage: null,
    importedAt: "2026-01-01T00:00:00Z",
    indexedAt: "2026-01-01T01:00:00Z",
    modifiedAt: "2026-01-01T00:30:00Z",
    entitiesStatus: "ok",
    entitiesError: null,
    cardStatus: "ok",
    cardType: "note",
    cardDate: "2026-01-01",
    cardPeople: ["Ada Chen"],
    cardOrganizations: ["Meridian Labs"],
  },
] as const;

export const E2E_DOCUMENT_CARD = {
  documentId: "e2e-doc-1",
  type: "note",
  title: "Fixture sample",
  date: "2026-01-01",
  people: [{ name: "Ada Chen", role: "author" }],
  organizations: ["Meridian Labs"],
  summary: [
    {
      text: "Ada Chen works at Meridian Labs.",
      quotes: [{ source: 1, text: "Ada Chen works at Meridian Labs." }],
    },
  ],
  facts: [
    {
      text: "Ada Chen works at Meridian Labs.",
      quotes: [{ source: 1, text: "Ada Chen works at Meridian Labs." }],
    },
  ],
  fields: [
    {
      key: "topic",
      value: "fixture",
      quotes: [{ source: 1, text: "Ada Chen works at Meridian Labs." }],
    },
  ],
  aliases: [],
  questions: ["What does Ada Chen do?"],
  model: "qwen3.5:4b",
  createdAt: "2026-01-01T02:00:00Z",
} as const;

export const E2E_SUGGESTED_QUESTIONS = [
  {
    question: "What does Ada Chen do?",
    scopeIds: ["e2e-doc-1"],
    label: null,
  },
] as const;


