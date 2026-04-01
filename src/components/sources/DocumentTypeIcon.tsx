import { cn } from "@/lib/utils";

type Props = { fileType: string; className?: string };

export function DocumentTypeIcon({ fileType, className }: Props) {
  const t = fileType.toLowerCase();
  const label = t === "pdf" ? "PDF" : t === "markdown" || t === "md" ? "MD" : t.slice(0, 4).toUpperCase();
  return (
    <span
      className={cn(
        "inline-flex h-6 w-6 shrink-0 items-center justify-center rounded border border-[var(--border)] font-mono text-[9px] font-semibold",
        t === "pdf" && "bg-red-950/40 text-red-200",
        (t === "markdown" || t === "md") && "bg-sky-950/40 text-sky-200",
        className,
      )}
      title={fileType}
      aria-hidden
    >
      {label}
    </span>
  );
}
