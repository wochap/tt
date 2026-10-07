import { ToggleGroup as T } from "radix-ui";
import type { ComponentProps, ReactNode } from "react";

import { cn } from "@/lib/utils";

/** Single-choice segmented control with the outlined active item of the design. */
export function Segmented<V extends string>({
  value,
  onValueChange,
  className,
  children,
  label,
}: {
  value: V;
  onValueChange: (value: V) => void;
  className?: string;
  children: ReactNode;
  label: string;
}) {
  return (
    <T.Root
      type="single"
      value={value}
      aria-label={label}
      onValueChange={(next) => next && onValueChange(next as V)}
      className={cn("tt-seg text-[12px]", className)}
    >
      {children}
    </T.Root>
  );
}

export function SegmentedItem({ className, ...props }: ComponentProps<typeof T.Item>) {
  return <T.Item data-seg-item="" className={cn("px-[11px] py-[5px] outline-none", className)} {...props} />;
}
