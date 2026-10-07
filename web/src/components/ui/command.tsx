import { Command as C } from "cmdk";
import type { ComponentProps } from "react";

import { cn } from "@/lib/utils";

// cmdk with filtering disabled: tt ranks with its own fuzzy index
// (`search` in @tt/domain) so palette, picker and tasks agree.

export function Command({ className, ...props }: ComponentProps<typeof C>) {
  return <C shouldFilter={false} loop className={cn("flex flex-col overflow-hidden text-fg", className)} {...props} />;
}

export function CommandInput({ className, ...props }: ComponentProps<typeof C.Input>) {
  return (
    <C.Input
      className={cn("min-w-0 flex-1 bg-transparent caret-accent outline-none placeholder:text-faint", className)}
      {...props}
    />
  );
}

export function CommandList({ className, ...props }: ComponentProps<typeof C.List>) {
  return <C.List className={cn("tt-scroll max-h-[360px] overflow-y-auto overscroll-contain p-[5px]", className)} {...props} />;
}

export function CommandEmpty({ className, ...props }: ComponentProps<typeof C.Empty>) {
  return <C.Empty className={cn("px-3 py-4 text-[12px] text-muted", className)} {...props} />;
}

export function CommandGroup({ className, ...props }: ComponentProps<typeof C.Group>) {
  return <C.Group className={cn("[&_[cmdk-group-heading]]:px-[9px] [&_[cmdk-group-heading]]:py-1 [&_[cmdk-group-heading]]:text-[10.5px] [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-[.08em] [&_[cmdk-group-heading]]:text-faint", className)} {...props} />;
}

export function CommandItem({ className, ...props }: ComponentProps<typeof C.Item>) {
  return (
    <C.Item
      className={cn(
        "flex h-8 cursor-default select-none items-center gap-2 rounded-2 px-[9px] outline-none data-[selected=true]:bg-[color-mix(in_srgb,var(--color-accent)_12%,transparent)] data-[selected=true]:shadow-[inset_0_0_0_1px_var(--color-accent)]",
        className,
      )}
      {...props}
    />
  );
}

export function CommandSeparator({ className, ...props }: ComponentProps<typeof C.Separator>) {
  return <C.Separator className={cn("mx-[6px] my-1 h-px bg-s0", className)} {...props} />;
}
