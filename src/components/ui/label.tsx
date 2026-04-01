import { cn } from "@/lib/utils";
import * as React from "react";

export type LabelProps = React.LabelHTMLAttributes<HTMLLabelElement>;

export function Label({ className, ...props }: LabelProps) {
  return (
    <label className={cn("text-xs font-medium leading-none text-[var(--foreground)]", className)} {...props} />
  );
}
