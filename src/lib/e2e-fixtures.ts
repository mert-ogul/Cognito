export const E2E_HEALTH = {
  sqliteOk: true,
  sidecarReachable: true,
  ollamaModelReady: true,
  vaultConfigured: true,
  chatModel: "phi4-mini",
  embedModel: "nomic-embed-text",
  chatModelWarmed: true,
  warmUpPending: false,
  warmUpError: null,
  ollamaReachable: true,
  missingModels: [] as string[],
  indexFormatUpdating: false,
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
    graphStatus: "ok",
    graphError: null,
  },
] as const;

export const E2E_GRAPH_SNAPSHOT = {
  nodes: [
    {
      nodeId: "n-e2e-1",
      name: "Sample Entity",
      kind: "concept",
      normalizedName: "sample entity",
      sourceCount: 2,
      confidenceAvg: 0.9,
    },
    {
      nodeId: "n-e2e-2",
      name: "Related Term",
      kind: "topic",
      normalizedName: "related term",
      sourceCount: 1,
      confidenceAvg: 0.7,
    },
  ],
  edges: [
    {
      edgeId: "e2e-e1",
      sourceNodeId: "n-e2e-1",
      targetNodeId: "n-e2e-2",
      relation: "relates_to",
      documentId: "e2e-doc-1",
      chunkId: "c1",
      confidence: 0.85,
    },
  ],
} as const;

export const E2E_NODE_DETAIL = {
  node: {
    nodeId: "n-e2e-1",
    name: "Sample Entity",
    kind: "concept",
    normalizedName: "sample entity",
    sourceCount: 2,
    confidenceAvg: 0.9,
    metadataJson: null,
    createdAt: "2026-01-01",
    updatedAt: "2026-01-01",
  },
  edges: E2E_GRAPH_SNAPSHOT.edges,
} as const;
