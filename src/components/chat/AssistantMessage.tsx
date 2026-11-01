import { Button } from "@/components/ui/button";
import { SourceQuoteOrThumb } from "@/components/common/SourceRegionThumb";
import {
  getDocumentPreview,
  type AnswerSentence,
  type CitationItem,
  type PageRegion,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { fileName, pageLabel } from "@/lib/format";
import { useEffect, useState, type ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";

export type ScopeSearchSuggestion = {
  documentId: string;
  title: string;
  label?: string | null;
};

export type AssistantChatMessage = {
  id: string;
  role: "assistant";
  text: string;
  citations?: CitationItem[];
  sentences?: AnswerSentence[];
  draft?: string[];
  notFound?: boolean;
  partial?: boolean;
  scopeTitle?: string | null;
  suggestions?: ScopeSearchSuggestion[];
  error?: string;
  stage?: "passages" | "ranking" | "writing" | "checking" | "done";
  stageDetail?: string;
};

export function citationSourceLabel(c: CitationItem): string {
  const name = (c.title || "").trim() || fileName(c.path);
  if (c.pageStart != null && c.pageEnd != null && c.pageEnd !== c.pageStart) {
    return `${name}, pp. ${c.pageStart}-${c.pageEnd}`;
  }
  if (c.pageStart != null) {
    return `${name}, p. ${c.pageStart}`;
  }
  return name;
}

/** Turn \\( \\) / \\[ \\] into $ / $$ outside code spans and blocks. */
export function normalizeAnswerMath(text: string): string {
  const parts = text.split(/(```[\s\S]*?```|`[^`\n]+`)/g);
  return parts
    .map((part) => {
      if (part.startsWith("`")) return part;
      return part
        .replace(/\\\(([\s\S]+?)\\\)/g, (_m, body: string) => `$${body}$`)
        .replace(/\\\[([\s\S]+?)\\\]/g, (_m, body: string) => `$$${body}$$`);
    })
    .join("");
}



/** Clamp snippet to at most three lines, starting at the best-matching sentence. */
function clampSnippetToMatch(text: string, query: string, maxLines = 3): string {
  const raw = (text || "").replace(/\s+/g, " ").trim();
  if (!raw) return "";
  const qTerms = new Set(
    (query || "")
      .toLowerCase()
      .match(/[0-9a-z]+(?:'[0-9a-z]+)?/g)
      ?.filter((t) => t.length >= 2) ?? [],
  );
  const sentences = raw.split(/(?<=[.!?])\s+/).filter(Boolean);
  let bestIdx = 0;
  let bestScore = -1;
  for (let i = 0; i < sentences.length; i++) {
    const terms = sentences[i].toLowerCase().match(/[0-9a-z]+(?:'[0-9a-z]+)?/g) ?? [];
    let score = 0;
    for (const t of terms) {
      if (qTerms.has(t)) score += 1;
    }
    if (score > bestScore) {
      bestScore = score;
      bestIdx = i;
    }
  }
  const fromBest = sentences.slice(bestIdx).join(" ");
  const words = fromBest.split(/\s+/);
  const approxLine = 14;
  const maxWords = approxLine * maxLines;
  if (words.length <= maxWords) return fromBest;
  return `${words.slice(0, maxWords).join(" ")}…`;
}

function hasDisplayMath(text: string): boolean {
  return /\$\$[\s\S]+?\$\$/.test(text);
}

const unwrapParagraph = {
  p: ({ children }: { children?: ReactNode }) => <>{children}</>,
};

async function enrichCitations(citations: CitationItem[]): Promise<CitationItem[]> {
  return Promise.all(
    citations.map(async (c) => {
      if (c.regions && c.regions.length > 0) return c;
      try {
        const prev = await getDocumentPreview(c.documentId, c.chunkId);
        const regions: PageRegion[] | null | undefined = prev.regions;
        if (!regions?.length && !prev.path) return c;
        return {
          ...c,
          regions: regions?.length ? regions : c.regions,
          path: prev.path || c.path,
          pageStart: prev.pageStart ?? c.pageStart,
          pageEnd: prev.pageEnd ?? c.pageEnd,
        };
      } catch {
        return c;
      }
    }),
  );
}

function PassageCard({
  citation: c,
  query,
  onCite,
  maxLines = 2,
  cropOnly = false,
}: {
  citation: CitationItem;
  query: string;
  onCite: (c: CitationItem) => void;
  maxLines?: number;
  /** Crop without the raw snippet under it; trimmed preview follows. */
  cropOnly?: boolean;
}) {
  const title = (c.title || "").trim() || fileName(c.path);
  const page = pageLabel(c.pageStart, c.pageEnd);
  const snippet = clampSnippetToMatch(c.snippet || "", query, maxLines);
  return (
    <li>
      <button
        type="button"
        className="w-full space-y-1 text-left text-muted hover:text-text"
        onClick={() => onCite(c)}
      >
        <p className="text-muted">
          <span>{title}</span>
          {page ? (
            <>
              <span aria-hidden="true"> · </span>
              <span>{page}</span>
            </>
          ) : null}
        </p>
        {c.regions && c.regions.length > 0 && c.documentId ? (
          <SourceQuoteOrThumb
            path={c.path}
            regions={c.regions}
            quote={cropOnly ? "" : c.snippet}
            documentId={c.documentId}
            title={title}
          />
        ) : null}
        {snippet ? <p className="text-muted">{snippet}</p> : null}
      </button>
    </li>
  );
}

function SourcesList({
  citations,
  citedIndexes,
  query,
  onCite,
}: {
  citations: CitationItem[];
  citedIndexes: Set<number>;
  query: string;
  onCite: (c: CitationItem) => void;
}) {
  const [enriched, setEnriched] = useState<CitationItem[]>(citations);
  const [expanded, setExpanded] = useState(false);
  const [showMore, setShowMore] = useState(false);

  useEffect(() => {
    setEnriched(citations);
    setExpanded(false);
    setShowMore(false);
    let cancelled = false;
    void (async () => {
      const next = await enrichCitations(citations);
      if (!cancelled) setEnriched(next);
    })();
    return () => {
      cancelled = true;
    };
  }, [citations]);

  if (citations.length === 0) return null;

  const cited = enriched.filter((c) => citedIndexes.has(c.index));
  const rest = enriched.filter((c) => !citedIndexes.has(c.index));
  const filterByCite = citedIndexes.size > 0;
  const primary = filterByCite ? cited : enriched;
  const extras = filterByCite ? rest : [];
  const count = primary.length > 0 ? primary.length : citations.length;

  return (
    <div className="space-y-2">
      <button
        type="button"
        className="text-left text-muted hover:text-text"
        onClick={() => {
          setExpanded((open) => {
            if (open) setShowMore(false);
            return !open;
          });
        }}
      >
        Sources · {count}
      </button>
      {expanded ? (
        <>
          {primary.length > 0 ? (
            <ul className="space-y-3">
              {primary.map((c) => (
                <PassageCard
                  key={`${c.chunkId}-${c.index}`}
                  citation={c}
                  query={query}
                  onCite={onCite}
                  maxLines={2}
                  cropOnly
                />
              ))}
            </ul>
          ) : null}
          {extras.length > 0 ? (
            showMore ? (
              <ul className="space-y-3">
                {extras.map((c) => (
                  <PassageCard
                    key={`${c.chunkId}-${c.index}`}
                    citation={c}
                    query={query}
                    onCite={onCite}
                    maxLines={2}
                    cropOnly
                  />
                ))}
              </ul>
            ) : (
              <button
                type="button"
                className="text-left text-muted hover:text-text"
                onClick={() => setShowMore(true)}
              >
                Show {extras.length} more passage{extras.length === 1 ? "" : "s"}
              </button>
            )
          ) : null}
        </>
      ) : null}
    </div>
  );
}

function ClosestPassages({
  citations,
  query,
  onCite,
}: {
  citations: CitationItem[];
  query: string;
  onCite: (c: CitationItem) => void;
}) {
  const closest = citations.slice(0, 2);
  const [enriched, setEnriched] = useState<CitationItem[]>(closest);

  useEffect(() => {
    setEnriched(closest);
    let cancelled = false;
    void (async () => {
      const next = await enrichCitations(closest);
      if (!cancelled) setEnriched(next);
    })();
    return () => {
      cancelled = true;
    };
  }, [citations]);

  if (closest.length === 0) return null;
  return (
    <details>
      <summary className="cursor-pointer text-muted">Closest passages</summary>
      <ul className="mt-2 space-y-3">
        {enriched.map((c) => (
          <PassageCard
            key={`${c.chunkId}-${c.index}`}
            citation={c}
            query={query}
            onCite={onCite}
            maxLines={2}
            cropOnly
          />
        ))}
      </ul>
    </details>
  );
}

function SentenceInline({
  sentence,
  citations,
  onCite,
}: {
  sentence: AnswerSentence;
  citations: CitationItem[];
  onCite: (c: CitationItem) => void;
}) {
  const byIndex = new Map(citations.map((c) => [c.index, c]));
  const unsupported = sentence.status === "unsupported";
  const block = hasDisplayMath(sentence.text);
  const Tag = block ? "div" : "span";
  return (
    <Tag
      className={cn(
        "answer font-read text-[15px] leading-[1.6]",
        block ? "my-2 block" : "inline",
        unsupported && "text-muted underline decoration-dotted underline-offset-2",
      )}
      title={unsupported ? "Not found in the sources" : undefined}
    >
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkMath]}
        rehypePlugins={[[rehypeKatex, { throwOnError: false }]]}
        components={unwrapParagraph}
      >
        {normalizeAnswerMath(sentence.text)}
      </ReactMarkdown>
      {sentence.sources.map((n) => {
        const c = byIndex.get(n);
        return (
          <sup key={n}>
            <button
              type="button"
              className="font-ui text-accent"
              onClick={() => {
                if (c) onCite(c);
              }}
              aria-label={`Source ${n}`}
            >
              {n}
            </button>
          </sup>
        );
      })}
      {block ? null : " "}
    </Tag>
  );
}

function AnswerParagraph({
  sentences,
  citations,
  onCite,
}: {
  sentences: AnswerSentence[];
  citations: CitationItem[];
  onCite: (c: CitationItem) => void;
}) {
  const formula = sentences.find((s) => s.reason === "formula");
  const formulaSource = formula?.sources?.[0];
  const weak = sentences.some((s) => s.reason === "weak");
  const unchecked = sentences.filter(
    (s) => s.status === "unchecked" && s.reason !== "formula" && s.reason !== "weak",
  );
  const uncheckedNote = weak
    ? "Parts of this couldn't be fully checked against the sources."
    : unchecked.length === 0
      ? null
      : sentences.length === 1
        ? "This couldn't be checked against the sources."
        : "Some sentences couldn't be checked.";
  return (
    <div className="space-y-1">
      <div className="answer font-read text-[15px] leading-[1.6] text-text">
        {sentences.map((s, i) => (
          <SentenceInline key={i} sentence={s} citations={citations} onCite={onCite} />
        ))}
      </div>
      {formula && formulaSource != null ? (
        <p className="text-[12px] text-muted">
          Rewritten from the PDF text. Open source {formulaSource} to compare it with the
          page.
        </p>
      ) : uncheckedNote ? (
        <p className="text-[12px] text-muted">{uncheckedNote}</p>
      ) : null}
    </div>
  );
}

export function AssistantMessage({
  message,
  query,
  onCite,
  onAskEverywhere,
  onSearchIn,
}: {
  message: AssistantChatMessage;
  query: string;
  onCite: (c: CitationItem) => void;
  onAskEverywhere: () => void;
  onSearchIn: (s: ScopeSearchSuggestion) => void;
}) {
  const citations = message.citations ?? [];
  const stage = message.stage;
  const statusLine =
    stage === "ranking"
      ? message.stageDetail || "Ranking passages"
      : stage === "checking"
        ? message.stageDetail || "Checking sentences"
        : stage === "writing" || stage === "passages"
          ? message.stageDetail || "Writing the answer"
          : null;

  const showFinal = stage === "done" && (message.sentences?.length ?? 0) > 0;
  const writing =
    stage === "passages" ||
    stage === "ranking" ||
    stage === "writing" ||
    stage === "checking";

  if (message.notFound) {
    const searchSuggestions = message.suggestions ?? [];
    return (
      <div className="space-y-3">
        <p className="text-muted">Your documents don&apos;t answer this.</p>
        <ClosestPassages citations={citations} query={query} onCite={onCite} />
        {searchSuggestions.length > 0 ? (
          <ul className="space-y-1">
            {searchSuggestions.map((s) => (
              <li key={s.documentId}>
                <button
                  type="button"
                  className="text-left text-muted hover:text-text"
                  onClick={() => onSearchIn(s)}
                >
                  {s.label || `Search in ${s.title}`}
                </button>
              </li>
            ))}
          </ul>
        ) : null}
      </div>
    );
  }

  if (writing && !showFinal) {
    return statusLine ? <p className="text-muted">{statusLine}</p> : null;
  }

  if (showFinal) {
    return (
      <div className="space-y-3">
        {message.scopeTitle ? (
          <p className="text-muted">
            Answered from {message.scopeTitle}{" "}
            <Button
              type="button"
              variant="quiet"
              className="h-auto px-1 py-0 text-muted underline-offset-2 hover:underline"
              onClick={onAskEverywhere}
            >
              Ask across everything
            </Button>
          </p>
        ) : null}
        <AnswerParagraph
          sentences={message.sentences ?? []}
          citations={citations}
          onCite={onCite}
        />
        {message.partial ? (
          <p className="text-muted">Your documents only cover part of this.</p>
        ) : null}
        {citations.length > 0 ? (
          <SourcesList
            citations={citations}
            citedIndexes={
              new Set((message.sentences ?? []).flatMap((s) => s.sources ?? []))
            }
            query={query}
            onCite={onCite}
          />
        ) : null}
      </div>
    );
  }

  return null;
}
