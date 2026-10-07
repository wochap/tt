import { Check } from "@phosphor-icons/react";
import { DropdownMenu as D } from "radix-ui";
import type { ComponentProps } from "react";

import { cn } from "@/lib/utils";

export const DropdownMenu = D.Root;
export const DropdownMenuTrigger = D.Trigger;
export const DropdownMenuGroup = D.Group;
export const DropdownMenuRadioGroup = D.RadioGroup;

export function DropdownMenuContent({ className, sideOffset = 6, align = "start", ...props }: ComponentProps<typeof D.Content>) {
  return (
    <D.Portal>
      <D.Content
        sideOffset={sideOffset}
        align={align}
        className={cn(
          "z-50 min-w-[180px] rounded-md bg-mantle p-[5px] text-[12.5px] text-fg shadow-[var(--shadow-lg)] data-[state=open]:animate-in data-[state=open]:fade-in-0",
          className,
        )}
        {...props}
      />
    </D.Portal>
  );
}

const itemClass =
  "relative flex h-[30px] cursor-default select-none items-center gap-2 rounded-2 px-[9px] outline-none data-[highlighted]:bg-[color-mix(in_srgb,var(--color-accent)_12%,transparent)] data-[highlighted]:shadow-[inset_0_0_0_1px_var(--color-accent)] data-[disabled]:opacity-45";

export function DropdownMenuItem({ className, ...props }: ComponentProps<typeof D.Item>) {
  return <D.Item className={cn(itemClass, className)} {...props} />;
}

export function DropdownMenuRadioItem({ className, children, ...props }: ComponentProps<typeof D.RadioItem>) {
  return (
    <D.RadioItem className={cn(itemClass, className)} {...props}>
      <span className="flex w-[14px] justify-center text-link">
        <D.ItemIndicator>
          <Check size={12} weight="bold" />
        </D.ItemIndicator>
      </span>
      {children}
    </D.RadioItem>
  );
}

export function DropdownMenuCheckboxItem({ className, children, ...props }: ComponentProps<typeof D.CheckboxItem>) {
  return (
    <D.CheckboxItem className={cn(itemClass, className)} onSelect={(e) => e.preventDefault()} {...props}>
      <span className="flex w-[14px] justify-center text-link">
        <D.ItemIndicator>
          <Check size={12} weight="bold" />
        </D.ItemIndicator>
      </span>
      {children}
    </D.CheckboxItem>
  );
}

export function DropdownMenuSeparator({ className, ...props }: ComponentProps<typeof D.Separator>) {
  return <D.Separator className={cn("mx-[6px] my-1 h-px bg-s0", className)} {...props} />;
}

export function DropdownMenuLabel({ className, ...props }: ComponentProps<typeof D.Label>) {
  return <D.Label className={cn("px-[9px] py-1 text-[11px] text-muted", className)} {...props} />;
}
