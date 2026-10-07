import { cva, type VariantProps } from "class-variance-authority";
import { Slot } from "radix-ui";
import type { ComponentProps } from "react";

import { cn } from "@/lib/utils";

// Nocturne buttons (outlined primary, divider-bordered secondary, ghost);
// states per the design's states sheet (frame 23).
const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md border border-transparent font-medium leading-[1.2] transition-colors disabled:pointer-events-none disabled:opacity-45 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        primary:
          "border-accent text-link hover:bg-[color-mix(in_srgb,var(--color-accent)_12%,transparent)] active:bg-[color-mix(in_srgb,var(--color-accent)_22%,transparent)]",
        secondary:
          "border-divider text-fg hover:bg-[color-mix(in_srgb,var(--c-text)_7%,transparent)] active:bg-[color-mix(in_srgb,var(--c-text)_14%,transparent)]",
        ghost:
          "text-link hover:bg-[color-mix(in_srgb,var(--color-accent)_10%,transparent)] active:bg-[color-mix(in_srgb,var(--color-accent)_18%,transparent)]",
        destructive:
          "text-red-fg hover:bg-[color-mix(in_srgb,var(--ctp-red)_10%,transparent)] active:bg-[color-mix(in_srgb,var(--ctp-red)_18%,transparent)]",
        running:
          "border-green text-green-fg hover:bg-[color-mix(in_srgb,var(--ctp-green)_12%,transparent)] active:bg-[color-mix(in_srgb,var(--ctp-green)_22%,transparent)]",
      },
      size: {
        sm: "px-3 py-[5px] text-[12.5px]",
        md: "px-[10px] py-[6px] text-[13px]",
        lg: "px-[18px] py-2 text-[14px]",
        icon: "size-[26px] rounded-2 p-0",
      },
    },
    defaultVariants: { variant: "secondary", size: "md" },
  },
);

export type ButtonProps = ComponentProps<"button"> & VariantProps<typeof buttonVariants> & { asChild?: boolean };

export function Button({ className, variant, size, asChild, type, ...props }: ButtonProps) {
  const Comp = asChild ? Slot.Root : "button";
  return (
    <Comp
      data-slot="button"
      type={asChild ? undefined : (type ?? "button")}
      className={cn(buttonVariants({ variant, size }), className)}
      {...props}
    />
  );
}

export { buttonVariants };
