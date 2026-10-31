import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import {
  ConnectionsPanel,
  filterAndSortEntities,
  groupTimelineByYear,
} from "./ConnectionsPanel";
import { useSidePaneStore } from "@/stores/sidePaneStore";
import { useMainViewStore } from "@/stores/mainViewStore";
import { useChatIntentStore } from "@/stores/chatIntentStore";
import type { EntityIndexItem, TimelineItem } from "@/lib/tauri";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

function wrap(ui: ReactNode) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(<QueryClientProvider client={qc}>{ui}</QueryClientProvider>);
}

const entities: EntityIndexItem[] = [
  {
    id: "e-person",
    name: "Ada Chen",
    normalized: "ada chen",
    label: "person",
    docCount: 2,
    mentionCount: 5,
  },
  {
    id: "e-org",
    name: "Meridian Labs",
    normalized: "meridian labs",
    label: "organization",
    docCount: 3,
    mentionCount: 4,
  },
  {
    id: "e-method",
    name: "GCN",
    normalized: "gcn",
    label: "method",
    docCount: 1,
    mentionCount: 2,
  },
  {
    id: "e-place",
    name: "Zurich",
    normalized: "zurich",
    label: "place",
    docCount: 1,
    mentionCount: 1,
  },
];

describe("ConnectionsPanel helpers", () => {
  it("filters by label and search, sorts by docCount then name", () => {
    const sorted = filterAndSortEntities(entities, "all", "");
    expect(sorted.map((e) => e.id)).toEqual(["e-org", "e-person", "e-method", "e-place"]);
    const people = filterAndSortEntities(entities, "person", "");
    expect(people.map((e) => e.id)).toEqual(["e-person"]);
    const search = filterAndSortEntities(entities, "all", "mer");
    expect(search.map((e) => e.id)).toEqual(["e-org"]);
  });

  it("groupTimelineByYear groups and sorts", () => {
    const items: TimelineItem[] = [
      { kind: "document", id: "d2", title: "B", date: "2021-05", documentId: "d2" },
      { kind: "entity", id: "e1", name: "Jan 2020", date: "2020-01", entityId: "e1" },
      { kind: "document", id: "d1", title: "A", date: "2020-06", documentId: "d1" },
    ];
    const groups = groupTimelineByYear(items);
    expect(groups.map((g) => g.year)).toEqual(["2020", "2021"]);
    expect(groups[0].items.map((i) => i.id)).toEqual(["e1", "d1"]);
  });
});

describe("ConnectionsPanel", () => {
  beforeEach(() => {
    useSidePaneStore.setState({
      selectedEntityId: null,
      mode: "connections",
      open: true,
      passage: null,
      highlightNodeIds: [],
    });
    useMainViewStore.setState({ view: "conversation", documentId: null });
    useChatIntentStore.setState({
      pendingScope: [],
      pendingQuestion: null,
      pendingRelated: null,
    });
    invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "get_entities_index") return entities;
      if (cmd === "get_entity_detail") {
        return {
          id: args?.entityId,
          name: "Ada Chen",
          label: "person",
          normalized: "ada chen",
          docCount: 2,
          mentionCount: 5,
          aliases: ["Ada"],
          documents: [
            {
              documentId: "doc-a",
              title: "Paper A",
              path: "/a.pdf",
              passages: [
                {
                  chunkId: "c1",
                  snippet: "Ada Chen proposed…",
                  pageStart: 1,
                  pageEnd: 1,
                },
              ],
            },
          ],
          coMentions: [
            { entityId: "e-org", name: "Meridian Labs", label: "organization", count: 3 },
          ],
        };
      }
      if (cmd === "get_connections_timeline") {
        return {
          items: [
            { kind: "document", id: "doc-a", title: "Paper A", date: "2020-01", documentId: "doc-a" },
            { kind: "entity", id: "e-date", name: "2021", date: "2021", entityId: "e-date" },
          ],
        };
      }
      return null;
    });
  });

  it("lists filtered entities and shows detail with co-mentions", async () => {
    wrap(<ConnectionsPanel />);
    await waitFor(() => expect(screen.getByText("Ada Chen")).toBeTruthy());
    expect(screen.getByText("Meridian Labs")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "People" }));
    expect(screen.queryByText("Meridian Labs")).toBeNull();
    expect(screen.getByText("Ada Chen")).toBeTruthy();

    fireEvent.click(screen.getByText("Ada Chen"));
    await waitFor(() => expect(screen.getByText("Often mentioned with")).toBeTruthy());
    expect(screen.getByText("Paper A")).toBeTruthy();
    expect(screen.getByText("Ada Chen proposed…")).toBeTruthy();
    expect(screen.getByRole("button", { name: "How are they related?" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "How are they related?" }));
    const related = useChatIntentStore.getState().pendingRelated;
    expect(related?.entityIdA).toBe("e-person");
    expect(related?.entityIdB).toBe("e-org");
  });

  it("timeline groups by year", async () => {
    wrap(<ConnectionsPanel />);
    fireEvent.click(screen.getByRole("button", { name: "Timeline" }));
    await waitFor(() => expect(screen.getByText("2020")).toBeTruthy());
    expect(screen.getAllByText("2021").length).toBeGreaterThan(0);
    expect(screen.getByText("Paper A")).toBeTruthy();
  });
});
