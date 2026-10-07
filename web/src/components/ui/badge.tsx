import type { CSSProperties, ComponentProps } from "react";

import { chipStyle } from "@/lib/colors";
import { cn } from "@/lib/utils";

/** Tinted chip for tags, projects, "running" and delta badges. */
export function Badge({
  color,
  textColor,
  strength,
  className,
  style,
  size = "sm",
  ...props
}: ComponentProps<"span"> & { color?: string; textColor?: string; strength?: number; size?: "xs" | "sm" | "md" }) {
  const merged: CSSProperties = { ...chipStyle(color, textColor, strength), ...style };
  return (
    <span
      className={cn(
        "inline-flex max-w-full items-center gap-[6px] truncate whitespace-nowrap rounded-2",
        size === "xs" && "px-[7px] py-px text-[10px]",
        size === "sm" && "px-[7px] py-[2px] text-[10.5px]",
        size === "md" && "px-[9px] py-[3px] text-[12px]",
        className,
      )}
      style={merged}
      {...props}
    />
  );
}

export function ColorDot({ color, size = 8, className }: { color: string; size?: number; className?: string }) {
  return <span className={cn("inline-block shrink-0 rounded-xs", className)} style={{ width: size, height: size, background: color }} />;
}
