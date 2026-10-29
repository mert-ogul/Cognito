import { Button } from "@/components/ui/button";
import { openDocument } from "@/lib/tauri";
import type { PassageView as PassageData } from "@/stores/sidePaneStore";
import { useEffect, useRef } from "react";

type Props = {
  passage: PassageData;
};

export function PassageView({ passage }: Props) {
  const markRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const el = markRef.current;
    if (el && typeof el.scrollIntoView === "function") {
      el.scrollIntoView({ block: "center" });
    }
  }, [passage.documentId, passage.highlight, passage.text]);

  const header =
    passage.pageLabel != null && passage.pageLabel.length > 0
      ? `${passage.title}, ${passage.pageLabel}`
      : passage.title;

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
      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3 font-read text-[14px]">
        <PassageBody
          text={passage.text}
          highlight={passage.highlight}
          markRef={markRef}
        />
      </div>
    </div>
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
