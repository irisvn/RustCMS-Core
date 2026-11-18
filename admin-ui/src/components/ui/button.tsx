import { cva, type VariantProps } from "class-variance-authority";
import type * as React from "react";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        // Disabled buttons change color, not opacity: a faded button stays
        // readable as a button in dark mode.
        default: "bg-primary text-primary-foreground hover:bg-primary-hover hover:ring-1 hover:ring-ring/40 active:bg-primary-hover disabled:bg-muted disabled:text-subtle-foreground",
        destructive: "bg-destructive text-destructive-foreground hover:bg-destructive-hover hover:ring-1 hover:ring-destructive/40 active:bg-destructive-hover disabled:bg-muted disabled:text-subtle-foreground",
        outline: "border border-input bg-card hover:bg-muted active:bg-selected disabled:border-border disabled:text-subtle-foreground",
        secondary: "bg-secondary text-secondary-foreground hover:bg-secondary-hover active:bg-secondary-hover disabled:bg-muted disabled:text-subtle-foreground",
        ghost: "hover:bg-muted active:bg-selected disabled:text-subtle-foreground",
        link: "text-primary underline-offset-4 hover:underline disabled:text-subtle-foreground",
      },
      size: {
        default: "h-9 px-4 py-2",
        sm: "h-8 rounded-md px-3 text-xs",
        lg: "h-10 rounded-md px-6",
        icon: "size-9",
      },
    },
    defaultVariants: { variant: "default", size: "default" },
  },
);

export type ButtonProps = React.ComponentProps<"button"> & VariantProps<typeof buttonVariants>;

export function Button({ className, variant, size, type = "button", ...props }: ButtonProps) {
  return <button type={type} className={cn(buttonVariants({ variant, size, className }))} {...props} />;
}

export { buttonVariants };
