import { cn } from "@/lib/utils";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import * as React from "react";

const variants = cva(
  "inline-flex items-center justify-center gap-1.5 rounded-[6px] text-[13px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-bg disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        primary: "bg-accent text-on-accent hover:opacity-90",
        secondary: "border border-line bg-transparent hover:bg-hover",
        quiet: "bg-transparent text-text hover:bg-hover",
      },
      size: {
        default: "h-7 px-2.5",
        sm: "h-7 px-2 text-[12px]",
      },
    },
    defaultVariants: {
      variant: "primary",
      size: "default",
    },
  },
);

export type ButtonProps = React.ButtonHTMLAttributes<HTMLButtonElement> &
  VariantProps<typeof variants> & { asChild?: boolean };

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return (
      <Comp className={cn(variants({ variant, size, className }))} ref={ref} {...props} />
    );
  },
);
Button.displayName = "Button";
