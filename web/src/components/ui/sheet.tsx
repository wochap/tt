import { Dialog as D } from "radix-ui";
import type { ComponentProps } from "react";

import { cn } from "@/lib/utils";

// Non-modal sheet anchored right (400 px): the timeline stays live behind it,
// so outside clicks do not close it; Esc does.

export const Sheet = (props: ComponentProps<typeof D.Root>) => <D.Root modal={false} {...props} />;
export const SheetTitle = D.Title;
export const SheetDescription = D.Description;
export const SheetClose = D.Close;

export function SheetContent({ className, ...props }: ComponentProps<typeof D.Content>) {
  return (
    <D.Portal>
      <D.Content
        onInteractOutside={(event) => event.preventDefault()}
        onOpenAutoFocus={(event) => event.preventDefault()}
        className={cn(
          "fixed bottom-0 right-0 top-0 z-40 flex w-[400px] max-w-full flex-col bg-mantle text-fg shadow-[var(--shadow-lg)] outline-none data-[state=open]:animate-in data-[state=open]:slide-in-from-right-8 data-[state=open]:fade-in-0",
          className,
        )}
        {...props}
      />
    </D.Portal>
  );
}
