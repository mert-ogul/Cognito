import { Button } from "@/components/ui/button";
import { fileName } from "@/lib/format";
import { listDocuments, type DocumentRow } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import type { ScopeDoc } from "@/stores/chatIntentStore";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type RefObject,
  type SetStateAction,
} from "react";

function atToken(value: string, cursor: number): { start: number; query: string } | null {
  const before = value.slice(0, cursor);
  const m = before.match(/@([^\s@]*)$/);
  if (!m) return null;
  return { start: before.length - m[0].length, query: m[1] ?? "" };
}

type Props = {
  input: string;
  setInput: (value: string) => void;
  scopeDocs: ScopeDoc[];
  setScopeDocs: Dispatch<SetStateAction<ScopeDoc[]>>;
  placeholder: string;
  disabled: boolean;
  onSend: () => void;
  textareaRef?: RefObject<HTMLTextAreaElement | null>;
};

export function ChatComposer({
  input,
  setInput,
  scopeDocs,
  setScopeDocs,
  placeholder,
  disabled,
  onSend,
  textareaRef: externalTextareaRef,
}: Props) {
  const [scopePickerOpen, setScopePickerOpen] = useState(false);
  const [atFilter, setAtFilter] = useState("");
  const [libraryDocs, setLibraryDocs] = useState<DocumentRow[]>([]);
  const composerRef = useRef<HTMLDivElement>(null);
  const internalTextareaRef = useRef<HTMLTextAreaElement>(null);
  const textareaRef = externalTextareaRef ?? internalTextareaRef;

  useLayoutEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    const line = 20;
    const max = line * 8 + 16;
    el.style.height = `${Math.min(el.scrollHeight, max)}px`;
  }, [input, textareaRef]);

  useEffect(() => {
    if (!scopePickerOpen) return;
    void listDocuments().then((docs) => {
      setLibraryDocs(docs.filter((d) => d.status === "indexed"));
    });
  }, [scopePickerOpen]);

  useEffect(() => {
    if (!scopePickerOpen) return;
    const onPointer = (e: MouseEvent) => {
      const root = composerRef.current;
      if (root && !root.contains(e.target as Node)) {
        setScopePickerOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointer);
    return () => document.removeEventListener("mousedown", onPointer);
  }, [scopePickerOpen]);

  const scopeChoices = useMemo(() => {
    const q = atFilter.trim().toLowerCase();
    const chosen = new Set(scopeDocs.map((d) => d.id));
    return libraryDocs
      .filter((d) => !chosen.has(d.id))
      .filter((d) => {
        if (!q) return true;
        const title = (d.title || fileName(d.path)).toLowerCase();
        return title.includes(q);
      })
      .slice(0, 8);
  }, [libraryDocs, scopeDocs, atFilter]);

  const pickScopeDoc = useCallback(
    (doc: DocumentRow) => {
      setScopeDocs((prev) =>
        prev.some((d) => d.id === doc.id)
          ? prev
          : [...prev, { id: doc.id, title: doc.title || fileName(doc.path) }],
      );
      const el = textareaRef.current;
      const cursor = el?.selectionStart ?? input.length;
      const token = atToken(input, cursor);
      if (token) {
        const next = input.slice(0, token.start) + input.slice(cursor);
        setInput(next);
      }
      setScopePickerOpen(false);
      setAtFilter("");
    },
    [input, setInput, setScopeDocs, textareaRef],
  );

  const onInputChange = useCallback(
    (value: string) => {
      setInput(value);
      const el = textareaRef.current;
      const cursor = el?.selectionStart ?? value.length;
      const token = atToken(value, cursor);
      if (token) {
        setScopePickerOpen(true);
        setAtFilter(token.query);
      } else {
        setScopePickerOpen(false);
        setAtFilter("");
      }
    },
    [setInput, textareaRef],
  );

  return (
    <div ref={composerRef} className="relative shrink-0 pb-4 pt-2">
      <div className="mb-1.5 flex flex-wrap items-center gap-x-2 gap-y-1">
        {scopeDocs.length === 0 ? (
          <p className="text-muted">Searching all documents</p>
        ) : (
          scopeDocs.map((d) => (
            <span key={d.id} className="inline-flex items-center gap-1 text-muted">
              In: {d.title}
              <Button
                type="button"
                variant="quiet"
                size="sm"
                className="h-auto px-1 py-0 text-muted"
                aria-label={`Remove ${d.title} from scope`}
                onClick={() => setScopeDocs((prev) => prev.filter((x) => x.id !== d.id))}
              >
                ×
              </Button>
            </span>
          ))
        )}
      </div>
      {scopePickerOpen && scopeChoices.length > 0 ? (
        <ul
          className="absolute bottom-full z-10 mb-1 max-h-48 w-full overflow-y-auto rounded-[8px] border border-line bg-bg py-1 shadow-sm"
          role="listbox"
          aria-label="Document scope"
        >
          {scopeChoices.map((d) => {
            const title = d.title || fileName(d.path);
            return (
              <li key={d.id}>
                <button
                  type="button"
                  role="option"
                  className="w-full px-3 py-1.5 text-left font-ui text-[14px] text-text hover:bg-hover"
                  onClick={() => pickScopeDoc(d)}
                >
                  {title}
                </button>
              </li>
            );
          })}
        </ul>
      ) : null}
      <textarea
        ref={textareaRef}
        rows={1}
        className={cn(
          "w-full resize-none overflow-y-auto rounded-[8px] border border-line bg-bg px-3 py-2 font-ui text-[15px] leading-[20px]",
          "placeholder:text-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent",
        )}
        placeholder={placeholder}
        value={input}
        disabled={disabled}
        onChange={(e) => onInputChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape" && scopePickerOpen) {
            e.preventDefault();
            setScopePickerOpen(false);
            return;
          }
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            onSend();
          }
        }}
        aria-label="Chat message"
      />
    </div>
  );
}
