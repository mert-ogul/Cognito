import { cn } from "@/lib/utils";
import { cva, type VariantProps } from "class-variance-authority";
import * as React from "react";

const variants = cva(
  "inline-flex items-center rounded-md border px-2 py-0.5 text-[10px] font-medium uppercase tracking-wide",
  {
    variants: {
      variant: {
        default: "border-[var(--border)] bg-[var(--surface)] text-[var(--foreground)]",
        success: "border-emerald-800/80 bg-emerald-950/50 text-emerald-200",
        warning: "border-amber-800/80 bg-amber-950/50 text-amber-200",
        danger: "border-red-800/80 bg-red-950/50 text-red-200",
        muted: "border-transparent bg-[var(--background)] text-[var(--muted)]",
      },
    },
    defaultVariants: { variant: "default" },
  },
);

export type BadgeProps = React.HTMLAttributes<HTMLSpanElement> & VariantProps<typeof variants>;

export function Badge({ className, variant, ...props }: BadgeProps) {
  return <span className={cn(variants({ variant }), className)} {...props} />;
}
