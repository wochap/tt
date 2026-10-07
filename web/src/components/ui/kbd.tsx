import type { ComponentProps } from "react";

import { cn } from "@/lib/utils";

/** Keyboard hint; `inherit` takes the color of the surrounding button. */
export function Kbd({ className, inherit, ...props }: ComponentProps<"kbd"> & { inherit?: boolean }) {
  return <kbd className={cn(inherit && "kbd-inherit", className)} {...props} />;
}
