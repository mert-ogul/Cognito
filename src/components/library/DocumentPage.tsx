import { SourceQuoteOrThumb } from "@/components/common/SourceRegionThumb";
import { Button } from "@/components/ui/button";
import {
  getDocumentCard,
  getDocumentPreview,
  getRelatedDocuments,
  getSettings,
  listDocuments,
  openDocument,
  renderDocumentPage,
  type DocumentCardPayload,
  type DocumentRow,
  type PageRegion,
  type RelatedDocument,
} from "@/lib/tauri";
import { useChatIntentStore } from "@/stores/chatIntentStore";
import { useCitedRegionsStore } from "@/stores/citedRegionsStore";
import { useMainViewStore } from "@/stores/mainViewStore";
import { useSidePaneStore } from "@/stores/sidePaneStore";
import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

type Props = {
  documentId: string;
  onBackToConversation: () => void;
  onAskAboutDocument: (scopeDoc: { id: string; title: string }) => void;
};

const EMPTY_REGIONS: PageRegion[] = [];

function fileName(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i >= 0 ? path.slice(i + 1) : path;
}

function pageLabel(pageStart: number | null | undefined, pageEnd: number | null | undefined): string | null {
  if (pageStart != null && pageEnd != null && pageEnd !== pageStart) {
    return `pp. ${pageStart}-${pageEnd}`;
  }
  if (pageStart != null) {
    return `p. ${pageStart}`;
  }
  return null;
}

function itemText(item: unknown): string {
  if (!item || typeof item !== "object") return "";
  const o = item as Record<string, unknown>;
  if (typeof o.text === "string" && o.text.trim()) return o.text.trim();
  if (typeof o.sentence === "string" && o.sentence.trim()) return o.sentence.trim();
  return "";
}

function firstQuote(item: unknown): { source: number; text: string } | null {
  if (!item || typeof item !== "object") return null;
  const quotes = (item as Record<string, unknown>).quotes;
  if (!Array.isArray(quotes) || quotes.length === 0) return null;
  const q = quotes[0];
  if (!q || typeof q !== "object") return null;
  const o = q as Record<string, unknown>;
  const text = typeof o.text === "string" ? o.text : "";
  const source = typeof o.source === "number" ? o.source : Number(o.source);
  if (!text.trim() || Number.isNaN(source)) return null;
  return { source, text };
}

function parseRegion(raw: unknown): PageRegion | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  const page = typeof o.page === "number" ? o.page : Number(o.page);
  const x0 = typeof o.x0 === "number" ? o.x0 : Number(o.x0);
  const y0 = typeof o.y0 === "number" ? o.y0 : Number(o.y0);
  const x1 = typeof o.x1 === "number" ? o.x1 : Number(o.x1);
  const y1 = typeof o.y1 === "number" ? o.y1 : Number(o.y1);
  if ([page, x0, y0, x1, y1].some((n) => Number.isNaN(n))) return null;
  return { page, x0, y0, x1, y1 };
}

function factRegions(item: unknown): PageRegion[] | null {
  if (!item || typeof item !== "object") return null;
  const o = item as Record<string, unknown>;
  const direct = o.regions;
  if (Array.isArray(direct)) {
    const parsed = direct.map(parseRegion).filter((r): r is PageRegion => !!r);
    if (parsed.length) return parsed;
  }
  const quotes = o.quotes;
  if (!Array.isArray(quotes) || quotes.length === 0) return null;
  const q = quotes[0];
  if (!q || typeof q !== "object") return null;
  const qr = (q as Record<string, unknown>).regions;
  if (!Array.isArray(qr)) return null;
  const parsed = qr.map(parseRegion).filter((r): r is PageRegion => !!r);
  return parsed.length ? parsed : null;
}

function personName(p: unknown): string {
  if (typeof p === "string") return p;
  if (p && typeof p === "object" && typeof (p as { name?: unknown }).name === "string") {
    return (p as { name: string }).name;
  }
  return "";
}

function fieldPair(item: unknown): { name: string; value: string } | null {
  if (!item || typeof item !== "object") return null;
  const o = item as Record<string, unknown>;
  const name =
    (typeof o.key === "string" && o.key) ||
    (typeof o.name === "string" && o.name) ||
    "";
  const value = typeof o.value === "string" ? o.value : "";
  if (!name && !value) return null;
  return { name, value };
}

export function DocumentPage({ documentId, onBackToConversation, onAskAboutDocument }: Props) {
  const openPassage = useSidePaneStore((s) => s.openPassage);
  const addCitedRegions = useCitedRegionsStore((s) => s.addRegions);
  const citedForDoc = useCitedRegionsStore((s) => s.byDoc[documentId]);
  const citedRegions = citedForDoc ?? EMPTY_REGIONS;
  const setAskAbout = useChatIntentStore((s) => s.setAskAbout);
  const setDocument = useMainViewStore((s) => s.setDocument);

  const docs = useQuery({
    queryKey: ["documents"],
    queryFn: listDocuments,
  });

  const settings = useQuery({
    queryKey: ["settings"],
    queryFn: getSettings,
  });

  const cardQ = useQuery({
    queryKey: ["document-card", documentId],
    queryFn: () => getDocumentCard(documentId),
  });

  const relatedQ = useQuery({
    queryKey: ["related-documents", documentId],
    queryFn: () => getRelatedDocuments(documentId),
  });

  const summariseOn = settings.data?.summariseDocuments !== false;

  const row: DocumentRow | undefined = useMemo(
    () => (docs.data ?? []).find((d) => d.id === documentId),
    [docs.data, documentId],
  );

  const card: DocumentCardPayload | null | undefined = cardQ.data;
  const title = (card?.title || row?.title || "").trim() || (row ? fileName(row.path) : "Document");
  const type = card?.type || row?.cardType || row?.fileType || null;
  const date = card?.date ?? row?.cardDate ?? null;
  const docPath = row?.path ?? "";
  const pageCount = row?.pageCount ?? null;

  const people = useMemo(() => {
    if (card?.people?.length) {
      return card.people.map(personName).filter(Boolean);
    }
    return row?.cardPeople ?? [];
  }, [card, row]);

  const organizations = useMemo(() => {
    if (card?.organizations?.length) return card.organizations;
    return row?.cardOrganizations ?? [];
  }, [card, row]);

  const summaryText = useMemo(() => {
    if (!card?.summary?.length) return "";
    return card.summary.map(itemText).filter(Boolean).join(" ");
  }, [card]);

  const facts = useMemo(() => {
    if (!card?.facts?.length) return [];
    return card.facts
      .map((f) => ({ text: itemText(f), quote: firstQuote(f), regions: factRegions(f) }))
      .filter((f) => f.text);
  }, [card]);

  const fields = useMemo(() => {
    if (!card?.fields?.length) return [];
    return card.fields.map(fieldPair).filter((f): f is { name: string; value: string } => !!f);
  }, [card]);

  const openQuote = useCallback(
    async (quoteText: string, regions?: PageRegion[] | null) => {
      const prev = await getDocumentPreview(documentId, null);
      const merged = (regions?.length ? regions : prev.regions) ?? null;
      const path = prev.path || docPath;
      const page = prev.pageStart ?? merged?.[0]?.page;
      addCitedRegions(documentId, merged);
      openPassage({
        documentId: prev.documentId,
        title,
        pageLabel: pageLabel(prev.pageStart, prev.pageEnd),
        text: prev.text,
        highlight: quoteText,
        path,
        page: page ?? undefined,
        pageCount,
        regions: merged,
      });
    },
    [addCitedRegions, docPath, documentId, openPassage, pageCount, title],
  );

  const askAbout = useCallback(() => {
    const scopeTitle =
      (card?.title || row?.title || "").trim() ||
      (row ? fileName(row.path) : title);
    const scope = { id: documentId, title: scopeTitle };
    setAskAbout([scope]);
    onAskAboutDocument(scope);
    onBackToConversation();
  }, [card, documentId, onAskAboutDocument, onBackToConversation, row, setAskAbout, title]);

  const askAboutBoth = useCallback(
    (other: RelatedDocument) => {
      const scopeTitle =
        (card?.title || row?.title || "").trim() ||
        (row ? fileName(row.path) : title);
      setAskAbout(
        [
          { id: documentId, title: scopeTitle },
          { id: other.id, title: other.title.trim() || other.id },
        ],
        "How do these two documents relate?",
      );
      onBackToConversation();
    },
    [card, documentId, onBackToConversation, row, setAskAbout, title],
  );

  const relatedCaption = (r: RelatedDocument): string => {
    if (r.kind === "mentions") return "Mentions this document";
    if (r.kind === "similar") return "Similar content";
    if (r.sharedEntities.length > 0) {
      return `Shares ${r.sharedEntities.join(", ")}`;
    }
    return "Shares names";
  };

  return (
    <div className="mx-auto flex h-full min-h-0 w-full max-w-[68ch] flex-col px-4 pt-6">
      <div className="mb-4 shrink-0">
        <button
          type="button"
          className="text-muted hover:text-text"
          onClick={onBackToConversation}
        >
          Conversation
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto pb-8">
        <h1 className="font-ui text-[20px] font-semibold text-text">{title}</h1>
        <p className="mt-1 text-muted">
          {[type, date].filter(Boolean).join(" · ")}
        </p>
        {people.length > 0 ? (
          <p className="mt-2 text-muted">People: {people.join(", ")}</p>
        ) : null}
        {organizations.length > 0 ? (
          <p className="mt-0.5 text-muted">Organizations: {organizations.join(", ")}</p>
        ) : null}

        {cardQ.isLoading ? <p className="mt-4 text-muted">Loading summary…</p> : null}
        {!cardQ.isLoading && !card ? (
          <p className="mt-4 text-muted">
            {row?.cardStatus === "failed"
              ? "Summary failed for this document."
              : summariseOn &&
                  (row?.cardStatus === "pending" || row?.cardStatus === "running")
                ? "Summarising…"
                : "No summary yet."}
          </p>
        ) : null}

        {summaryText ? <p className="mt-4 text-text">{summaryText}</p> : null}

        {facts.length > 0 ? (
          <div className="mt-6 space-y-2">
            <p className="font-medium text-text">Key facts</p>
            <ul className="list-disc space-y-3 pl-5">
              {facts.map((f, i) => (
                <li key={`${f.text}-${i}`} className="text-text">
                  <span>{f.text}</span>
                  {f.quote ? (
                    <div className="mt-1">
                      {f.regions && f.regions.length > 0 ? (
                        <SourceQuoteOrThumb
                          path={docPath}
                          regions={f.regions}
                          quote={f.quote.text}
                          documentId={documentId}
                          title={title}
                          pageCount={pageCount}
                        />
                      ) : (
                        <button
                          type="button"
                          className="text-muted hover:text-text"
                          onClick={() => void openQuote(f.quote!.text, f.regions)}
                        >
                          source
                        </button>
                      )}
                    </div>
                  ) : null}
                </li>
              ))}
            </ul>
          </div>
        ) : null}

        {fields.length > 0 ? (
          <div className="mt-6 space-y-2">
            <p className="font-medium text-text">Fields</p>
            <dl className="grid grid-cols-[minmax(0,1fr)_minmax(0,2fr)] gap-x-4 gap-y-1">
              {fields.map((f, i) => (
                <div key={`${f.name}-${i}`} className="contents">
                  <dt className="text-muted">{f.name}</dt>
                  <dd className="text-text">{f.value}</dd>
                </div>
              ))}
            </dl>
          </div>
        ) : null}

        <div className="mt-8 flex flex-wrap gap-2">
          <Button type="button" variant="primary" onClick={askAbout}>
            Ask about this document
          </Button>
          <Button
            type="button"
            variant="quiet"
            onClick={() => void openDocument(documentId)}
          >
            Open file
          </Button>
        </div>

        {(relatedQ.data?.length ?? 0) > 0 ? (
          <div className="mt-8 space-y-2">
            <p className="font-medium text-text">Related documents</p>
            <ul className="flex flex-col gap-2">
              {(relatedQ.data ?? []).map((r) => (
                <li key={r.id} className="flex flex-col gap-0.5">
                  <button
                    type="button"
                    className="text-left text-[13px] text-text hover:underline"
                    onClick={() => setDocument(r.id)}
                  >
                    {r.title.trim() || r.id}
                  </button>
                  <p className="text-[12px] text-muted">{relatedCaption(r)}</p>
                  <button
                    type="button"
                    className="w-fit text-[12px] text-muted hover:text-text"
                    onClick={() => askAboutBoth(r)}
                  >
                    Ask about both
                  </button>
                </li>
              ))}
            </ul>
          </div>
        ) : null}

        {pageCount != null && pageCount > 0 ? (
          <DocumentPages
            documentId={documentId}
            pageCount={pageCount}
            citedRegions={citedRegions}
          />
        ) : null}
      </div>
    </div>
  );
}

function DocumentPages({
  documentId,
  pageCount,
  citedRegions,
}: {
  documentId: string;
  pageCount: number;
  citedRegions: PageRegion[];
}) {
  const pages = useMemo(
    () => Array.from({ length: pageCount }, (_, i) => i + 1),
    [pageCount],
  );
  const byPage = useMemo(() => {
    const map = new Map<number, PageRegion[]>();
    for (const r of citedRegions) {
      const list = map.get(r.page) ?? [];
      list.push(r);
      map.set(r.page, list);
    }
    return map;
  }, [citedRegions]);

  return (
    <div className="mt-10 space-y-3">
      <h2 className="font-medium text-text">Pages</h2>
      <div className="space-y-4">
        {pages.map((page) => (
          <DocumentPageImage
            key={page}
            documentId={documentId}
            page={page}
            regions={byPage.get(page) ?? []}
          />
        ))}
      </div>
    </div>
  );
}

function DocumentPageImage({
  documentId,
  page,
  regions,
}: {
  documentId: string;
  page: number;
  regions: PageRegion[];
}) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [visible, setVisible] = useState(false);
  const [img, setImg] = useState<{
    src: string;
    width: number;
    height: number;
    scale: number;
  } | null>(null);

  useEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    if (typeof IntersectionObserver === "undefined") {
      setVisible(true);
      return;
    }
    const obs = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setVisible(true);
          obs.disconnect();
        }
      },
      { rootMargin: "120px" },
    );
    obs.observe(el);
    return () => obs.disconnect();
  }, []);

  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    const scale = 1.5;
    void (async () => {
      try {
        const rendered = await renderDocumentPage(documentId, page, scale);
        if (cancelled) return;
        setImg({
          src: `data:image/png;base64,${rendered.pngBase64}`,
          width: rendered.width,
          height: rendered.height,
          scale,
        });
      } catch {
        if (!cancelled) setImg(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [visible, documentId, page]);

  return (
    <div ref={rootRef} className="space-y-1">
      <p className="text-muted">Page {page}</p>
      {img ? (
        <div
          className="relative inline-block max-w-full"
          style={{ width: img.width, maxWidth: "100%" }}
        >
          <img
            src={img.src}
            alt={`Page ${page}`}
            width={img.width}
            height={img.height}
            className="h-auto max-w-full"
          />
          {regions.map((r, i) => {
            const left = (r.x0 * img.scale) / img.width;
            const top = (r.y0 * img.scale) / img.height;
            const width = ((r.x1 - r.x0) * img.scale) / img.width;
            const height = ((r.y1 - r.y0) * img.scale) / img.height;
            return (
              <div
                key={`${r.x0}-${r.y0}-${i}`}
                aria-hidden
                className="pointer-events-none absolute border-2 border-accent bg-accent/15"
                style={{
                  left: `${left * 100}%`,
                  top: `${top * 100}%`,
                  width: `${width * 100}%`,
                  height: `${height * 100}%`,
                }}
              />
            );
          })}
        </div>
      ) : (
        <div className="h-40 rounded-[6px] bg-hover" aria-hidden />
      )}
    </div>
  );
}
