import { Button } from "@/components/ui/button";
import { openDocument, renderDocumentPage, type PageRegion } from "@/lib/tauri";
import type { PassageView as PassageData } from "@/stores/sidePaneStore";
import { useEffect, useMemo, useRef, useState } from "react";

type Props = {
  passage: PassageData;
};

function initialPage(passage: PassageData): number {
  if (passage.page != null && passage.page > 0) return passage.page;
  const fromRegion = passage.regions?.find((r) => r.page > 0)?.page;
  if (fromRegion != null) return fromRegion;
  return 1;
}

function regionsOnPage(regions: PageRegion[] | null | undefined, page: number): PageRegion[] {
  if (!regions) return [];
  return regions.filter((r) => r.page === page);
}

export function PassageView({ passage }: Props) {
  const markRef = useRef<HTMLElement | null>(null);
  const [page, setPage] = useState(() => initialPage(passage));
  const [pageCount, setPageCount] = useState<number | null>(passage.pageCount ?? null);
  const [img, setImg] = useState<{
    src: string;
    width: number;
    height: number;
    scale: number;
  } | null>(null);
  const [textOpen, setTextOpen] = useState(false);
  const path = passage.path;
  const showPage = Boolean(path && passage.regions && passage.regions.length > 0);
  const pageRegions = useMemo(
    () => regionsOnPage(passage.regions, page),
    [passage.regions, page],
  );

  useEffect(() => {
    setPage(initialPage(passage));
    setPageCount(passage.pageCount ?? null);
    setTextOpen(false);
    setImg(null);
  }, [passage.documentId, passage.path, passage.page, passage.pageCount, passage.regions]);

  useEffect(() => {
    const el = markRef.current;
    if (el && typeof el.scrollIntoView === "function") {
      el.scrollIntoView({ block: "center" });
    }
  }, [passage.documentId, passage.highlight, passage.text, textOpen]);

  useEffect(() => {
    if (!showPage || !path) return;
    let cancelled = false;
    const scale = 2;
    void (async () => {
      try {
        const rendered = await renderDocumentPage(path, page, scale);
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
  }, [showPage, path, page]);

  const header =
    passage.pageLabel != null && passage.pageLabel.length > 0
      ? `${passage.title}, ${passage.pageLabel}`
      : passage.title;

  const maxPage = pageCount != null && pageCount > 0 ? pageCount : null;
  const canPrev = page > 1;
  const canNext = maxPage == null ? true : page < maxPage;

  const goPrev = () => {
    setPage((p) => Math.max(1, p - 1));
  };
  const goNext = () => {
    setPage((p) => (maxPage != null ? Math.min(maxPage, p + 1) : p + 1));
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 items-start justify-between gap-2 border-b border-line px-3 py-2">
        <p className="min-w-0 flex-1 font-medium text-text">{header}</p>
        <Button
          type="button"
          variant="quiet"
          onClick={() => void openDocument(passage.documentId)}
        >
          Open file
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
        {showPage ? (
          <div className="space-y-3">
            <div className="flex items-center justify-between gap-2">
              <Button type="button" variant="quiet" disabled={!canPrev} onClick={goPrev}>
                Previous page
              </Button>
              <p className="text-muted">
                Page {page}
                {maxPage != null ? ` of ${maxPage}` : ""}
              </p>
              <Button type="button" variant="quiet" disabled={!canNext} onClick={goNext}>
                Next page
              </Button>
            </div>
            {img ? (
              <div
                className="relative mx-auto inline-block max-w-full"
                style={{ width: img.width, maxWidth: "100%" }}
              >
                <img
                  src={img.src}
                  alt={`Page ${page}`}
                  width={img.width}
                  height={img.height}
                  className="h-auto max-w-full"
                />
                {pageRegions.map((r, i) => (
                  <RegionOutline
                    key={`${r.page}-${r.x0}-${r.y0}-${i}`}
                    region={r}
                    imgWidth={img.width}
                    imgHeight={img.height}
                    scale={img.scale}
                  />
                ))}
              </div>
            ) : (
              <p className="text-muted">Loading page…</p>
            )}
            <details
              open={textOpen}
              onToggle={(e) => setTextOpen((e.target as HTMLDetailsElement).open)}
            >
              <summary className="cursor-pointer text-muted hover:text-text">Text</summary>
              <div className="mt-2 font-read text-[14px]">
                <PassageBody
                  text={passage.text}
                  highlight={passage.highlight}
                  markRef={markRef}
                />
              </div>
            </details>
          </div>
        ) : (
          <div className="font-read text-[14px]">
            <PassageBody
              text={passage.text}
              highlight={passage.highlight}
              markRef={markRef}
            />
          </div>
        )}
      </div>
    </div>
  );
}

function RegionOutline({
  region,
  imgWidth,
  imgHeight,
  scale,
}: {
  region: PageRegion;
  imgWidth: number;
  imgHeight: number;
  scale: number;
}) {
  // PyMuPDF page origin is top-left; y grows down; pixmap matches page coords × scale.
  const left = (region.x0 * scale) / imgWidth;
  const top = (region.y0 * scale) / imgHeight;
  const width = ((region.x1 - region.x0) * scale) / imgWidth;
  const height = ((region.y1 - region.y0) * scale) / imgHeight;
  return (
    <div
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
}

function PassageBody({
  text,
  highlight,
  markRef,
}: {
  text: string | null;
  highlight: string | null;
  markRef: React.MutableRefObject<HTMLElement | null>;
}) {
  if (!text) {
    return <p className="text-muted">No preview text.</p>;
  }
  if (!highlight) {
    return <p className="whitespace-pre-wrap text-text">{text}</p>;
  }
  const parts = text.split("\n\n");
  const citedIdx = parts.findIndex((p) => p === highlight);
  if (citedIdx < 0) {
    const i = text.indexOf(highlight);
    if (i < 0) {
      return <p className="whitespace-pre-wrap text-muted">{text}</p>;
    }
    const before = text.slice(0, i);
    const mid = text.slice(i, i + highlight.length);
    const after = text.slice(i + highlight.length);
    return (
      <p className="whitespace-pre-wrap">
        <span className="text-muted">{before}</span>
        <mark
          ref={(el) => {
            markRef.current = el;
          }}
          className="bg-mark text-text"
        >
          {mid}
        </mark>
        <span className="text-muted">{after}</span>
      </p>
    );
  }
  return (
    <div className="space-y-3">
      {parts.map((part, i) => {
        if (i === citedIdx) {
          return (
            <p
              key={i}
              ref={(el) => {
                markRef.current = el;
              }}
              className="whitespace-pre-wrap bg-mark text-text"
            >
              {part}
            </p>
          );
        }
        return (
          <p key={i} className="whitespace-pre-wrap text-muted">
            {part}
          </p>
        );
      })}
    </div>
  );
}
