import { Tooltip as T } from "radix-ui";
import type { ComponentProps, ReactNode } from "react";

export const TooltipProvider = T.Provider;

export function Tooltip({ content, children, side = "top", ...props }: { content: ReactNode; children: ReactNode } & Omit<ComponentProps<typeof T.Content>, "content">) {
  if (!content) return <>{children}</>;
  return (
    <T.Root>
      <T.Trigger asChild>{children}</T.Trigger>
      <T.Portal>
        <T.Content
          side={side}
          sideOffset={6}
          className="z-50 max-w-[320px] rounded-sm bg-mantle px-2 py-1 text-[11.5px] text-fg shadow-[var(--shadow-md)] data-[state=delayed-open]:animate-in data-[state=delayed-open]:fade-in-0"
          {...props}
        >
          {content}
        </T.Content>
      </T.Portal>
    </T.Root>
  );
}
