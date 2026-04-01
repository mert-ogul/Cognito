import { cn } from "@/lib/utils";

type Props = {
  value: number;
  className?: string;
  "aria-label"?: string;
};

/** Determinate progress 0–100. */
export function Progress({ value, className, ...rest }: Props) {
  const v = Math.min(100, Math.max(0, value));
  return (
    <div
      role="progressbar"
      aria-valuenow={v}
      aria-valuemin={0}
      aria-valuemax={100}
      className={cn("h-2 w-full overflow-hidden rounded-full bg-[var(--background)]", className)}
      {...rest}
    >
      <div className="h-full rounded-full bg-[var(--accent)] transition-[width] duration-300" style={{ width: `${v}%` }} />
    </div>
  );
}
