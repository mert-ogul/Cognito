import { renderPageRegion, type PageRegion } from "@/lib/tauri";
import { useCitedRegionsStore } from "@/stores/citedRegionsStore";
import { useSidePaneStore } from "@/stores/sidePaneStore";
import { useEffect, useRef, useState } from "react";

type Props = {
  path: string;
  region: PageRegion;
  quote: string;
  documentId?: string;
  title?: string;
  pageCount?: number | null;
};

export function SourceRegionThumb({
  path,
  region,
  quote,
  documentId,
  title,
  pageCount,
}: Props) {
  const rootRef = useRef<HTMLButtonElement | null>(null);
  const [visible, setVisible] = useState(false);
  const [img, setImg] = useState<{ src: string; width: number; height: number } | null>(null);
  const [failed, setFailed] = useState(false);
  const openPassage = useSidePaneStore((s) => s.openPassage);
  const addRegions = useCitedRegionsStore((s) => s.addRegions);

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
      { rootMargin: "80px" },
    );
    obs.observe(el);
    return () => obs.disconnect();
  }, []);

  useEffect(() => {
    if (!visible || !path) return;
    let cancelled = false;
    void (async () => {
      try {
        const rendered = await renderPageRegion(path, region.page, {
          x0: region.x0,
          y0: region.y0,
          x1: region.x1,
          y1: region.y1,
        });
        if (cancelled) return;
        setImg({
          src: `data:image/png;base64,${rendered.pngBase64}`,
          width: rendered.width,
          height: rendered.height,
        });
      } catch {
        if (!cancelled) setFailed(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [visible, path, region.page, region.x0, region.y0, region.x1, region.y1]);

  const open = () => {
    if (documentId) {
      addRegions(documentId, [region]);
    }
    openPassage({
      documentId: documentId ?? "",
      title: title ?? "",
      pageLabel: `p. ${region.page}`,
      text: quote,
      highlight: quote,
      path,
      page: region.page,
      pageCount: pageCount ?? null,
      regions: [region],
    });
  };

  return (
    <button
      ref={rootRef}
      type="button"
      className="block max-w-full text-left"
      onClick={open}
      aria-label="Open source region"
    >
      {img && !failed ? (
        <img
          src={img.src}
          alt="Source region"
          width={img.width}
          height={img.height}
          className="mb-1 max-h-[220px] w-auto max-w-full object-contain"
        />
      ) : null}
      <span className="text-muted">{quote}</span>
    </button>
  );
}

type QuoteOrThumbProps = {
  path?: string | null;
  regions?: PageRegion[] | null;
  quote: string;
  documentId?: string;
  title?: string;
  pageCount?: number | null;
};

/** Prefer a region crop when available; otherwise muted quote text. */
export function SourceQuoteOrThumb({
  path,
  regions,
  quote,
  documentId,
  title,
  pageCount,
}: QuoteOrThumbProps) {
  const region = regions?.[0];
  if (path && region) {
    return (
      <SourceRegionThumb
        path={path}
        region={region}
        quote={quote}
        documentId={documentId}
        title={title}
        pageCount={pageCount}
      />
    );
  }
  return <p className="text-muted">{quote}</p>;
}
