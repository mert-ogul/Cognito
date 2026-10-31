import { Button } from "@/components/ui/button";
import { SourceQuoteOrThumb } from "@/components/common/SourceRegionThumb";
import {
  getDocumentPreview,
  type AnswerSentence,
  type CitationItem,
  type PageRegion,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
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

/** Clamp snippet to at most three lines, starting at the best-matching sentence. */
export function clampSnippetToMatch(text: string, query: string, maxLines = 3): string {
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

function SourcesList({
  citations,
  onCite,
}: {
  citations: CitationItem[];
  onCite: (c: CitationItem) => void;
}) {
  const [enriched, setEnriched] = useState<CitationItem[]>(citations);

  useEffect(() => {
    setEnriched(citations);
    let cancelled = false;
    void (async () => {
      const next = await Promise.all(
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
      if (!cancelled) setEnriched(next);
    })();
    return () => {
      cancelled = true;
    };
  }, [citations]);

  if (citations.length === 0) return null;
  return (
    <div className="space-y-2">
      <p className="text-muted">Sources</p>
      <ol className="list-decimal space-y-2 pl-5 text-muted">
        {enriched.map((c) => (
          <li key={`${c.chunkId}-${c.index}`} className="space-y-1">
            {c.regions && c.regions.length > 0 && c.documentId ? (
              <SourceQuoteOrThumb
                path={c.path}
                regions={c.regions}
                quote={c.snippet}
                documentId={c.documentId}
                title={(c.title || "").trim() || fileName(c.path)}
              />
            ) : (
              <p className="text-muted">{c.snippet}</p>
            )}
            <button
              type="button"
              className="text-left text-muted hover:text-text"
              onClick={() => onCite(c)}
            >
              {citationSourceLabel(c)}
            </button>
          </li>
        ))}
      </ol>
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
  const [enriched, setEnriched] = useState<CitationItem[]>(citations);

  useEffect(() => {
    setEnriched(citations);
    let cancelled = false;
    void (async () => {
      const next = await Promise.all(
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
      if (!cancelled) setEnriched(next);
    })();
    return () => {
      cancelled = true;
    };
  }, [citations]);

  if (citations.length === 0) return null;
  return (
    <ul className="space-y-3">
      {enriched.map((c) => {
        const title = (c.title || "").trim() || fileName(c.path);
        const page = pageLabel(c.pageStart, c.pageEnd);
        const snippet = clampSnippetToMatch(c.snippet || "", query);
        return (
          <li key={`${c.chunkId}-${c.index}`}>
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
              {snippet ? <p className="text-muted">{snippet}</p> : null}
              {c.regions && c.regions.length > 0 && c.documentId ? (
                <SourceQuoteOrThumb
                  path={c.path}
                  regions={c.regions}
                  quote={c.snippet}
                  documentId={c.documentId}
                  title={title}
                />
              ) : null}
            </button>
          </li>
        );
      })}
    </ul>
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
  muted,
}: {
  sentences: AnswerSentence[];
  citations: CitationItem[];
  onCite: (c: CitationItem) => void;
  muted?: boolean;
}) {
  const hasFormula = sentences.some((s) => s.reason === "formula");
  const hasUnchecked = sentences.some((s) => s.status === "unchecked");
  return (
    <div className={cn("space-y-1", muted && "text-muted")}>
      <div className={cn("answer font-read text-[15px] leading-[1.6]", muted ? "text-muted" : "text-text")}>
        {sentences.map((s, i) => (
          <SentenceInline key={i} sentence={s} citations={citations} onCite={onCite} />
        ))}
      </div>
      {hasFormula ? (
        <p className="text-[12px] text-muted">
          Formulas are shown as written by the model; compare them with the page images below.
        </p>
      ) : hasUnchecked ? (
        <p className="text-[12px] text-muted">Some sentences couldn&apos;t be checked.</p>
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

  const draftSentences: AnswerSentence[] = (message.draft ?? []).map((text) => ({
    text,
    status: "unchecked",
    sources: [],
  }));
  const showDraft =
    (stage === "writing" || stage === "checking") && draftSentences.length > 0;
  const showFinal = stage === "done" && (message.sentences?.length ?? 0) > 0;

  if (message.notFound) {
    const searchSuggestions = message.suggestions ?? [];
    const drafted = message.sentences ?? [];
    return (
      <div className="space-y-3">
        <p className="text-muted">Your documents don&apos;t answer this.</p>
        <ClosestPassages citations={citations} query={query} onCite={onCite} />
        {drafted.length > 0 ? (
          <details>
            <summary className="cursor-pointer text-muted">Show the unconfirmed answer</summary>
            <div className="mt-2">
              <AnswerParagraph
                sentences={drafted}
                citations={citations}
                onCite={onCite}
                muted
              />
            </div>
          </details>
        ) : null}
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

  if (showFinal || showDraft || statusLine || citations.length > 0) {
    return (
      <div className="space-y-3">
        {message.scopeTitle && showFinal ? (
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
        {showFinal ? (
          <AnswerParagraph
            sentences={message.sentences ?? []}
            citations={citations}
            onCite={onCite}
          />
        ) : showDraft ? (
          <AnswerParagraph
            sentences={draftSentences}
            citations={citations}
            onCite={onCite}
            muted
          />
        ) : statusLine ? (
          <p className="text-muted">{statusLine}</p>
        ) : null}
        {citations.length > 0 ? <SourcesList citations={citations} onCite={onCite} /> : null}
      </div>
    );
  }

  return null;
}
