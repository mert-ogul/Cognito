import { describe, expect, it } from "vitest";
import { conversationTitle, storedMessageToChatMessage } from "./chatMessage";

describe("conversationTitle", () => {
  it("collapses whitespace and truncates at a word boundary", () => {
    expect(conversationTitle("  hello   world  ")).toBe("hello world");
    const long =
      "one two three four five six seven eight nine ten eleven twelve thirteen fourteen";
    const t = conversationTitle(long);
    expect(t.endsWith("…")).toBe(true);
    expect(t.length).toBeLessThanOrEqual(61);
  });
});

describe("storedMessageToChatMessage", () => {
  it("maps done assistant payload", () => {
    const m = storedMessageToChatMessage({
      id: "a1",
      conversationId: "c1",
      position: 1,
      role: "assistant",
      status: "done",
      content: "Hello.",
      payload: {
        citations: [],
        sentences: [{ text: "Hello.", status: "supported", sources: [] }],
        notFound: false,
        scopeTitle: null,
        suggestions: [],
        error: null,
      },
      createdAt: "2026-01-01T00:00:00Z",
    });
    expect(m.role).toBe("assistant");
    expect(m.stage).toBe("done");
    expect(m.sentences?.[0]?.text).toBe("Hello.");
    expect(m.partial).toBe(false);
  });

  it("keeps partial after reload from stored payload", () => {
    const m = storedMessageToChatMessage({
      id: "a2",
      conversationId: "c1",
      position: 2,
      role: "assistant",
      status: "done",
      content: "Seven crews are listed.",
      payload: {
        citations: [],
        sentences: [
          { text: "Seven crews are listed.", status: "supported", sources: [1] },
        ],
        notFound: false,
        partial: true,
        scopeTitle: null,
        suggestions: [],
        error: null,
      },
      createdAt: "2026-01-01T00:00:00Z",
    });
    expect(m.partial).toBe(true);
  });

  it("maps interrupted pending cleanup", () => {
    const m = storedMessageToChatMessage({
      id: "a1",
      conversationId: "c1",
      position: 1,
      role: "assistant",
      status: "error",
      content: "This answer was interrupted.",
      payload: { error: "This answer was interrupted." },
      createdAt: "2026-01-01T00:00:00Z",
    });
    expect(m.error).toBe("This answer was interrupted.");
  });
});
