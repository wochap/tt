import { CaretDown, Check } from "@phosphor-icons/react";
import { Select as S } from "radix-ui";
import type { ComponentProps, ReactNode } from "react";

import { cn } from "@/lib/utils";

export function Select({
  value,
  onValueChange,
  children,
  className,
  label,
  display,
}: {
  value: string;
  onValueChange: (value: string) => void;
  children: ReactNode;
  className?: string;
  label: string;
  display?: ReactNode;
}) {
  return (
    <S.Root value={value} onValueChange={onValueChange}>
      <S.Trigger aria-label={label} className={cn("tt-input justify-between gap-2 text-[13px]", className)}>
        <S.Value>{display}</S.Value>
        <S.Icon className="text-faint">
          <CaretDown size={12} />
        </S.Icon>
      </S.Trigger>
      <S.Portal>
        <S.Content
          position="popper"
          sideOffset={4}
          className="z-50 max-h-[320px] min-w-[var(--radix-select-trigger-width)] overflow-hidden rounded-md bg-mantle text-[12.5px] text-fg shadow-[var(--shadow-lg)]"
        >
          <S.Viewport className="tt-scroll p-[5px]">{children}</S.Viewport>
        </S.Content>
      </S.Portal>
    </S.Root>
  );
}

export function SelectItem({ className, children, ...props }: ComponentProps<typeof S.Item>) {
  return (
    <S.Item
      className={cn(
        "flex h-[30px] cursor-default select-none items-center gap-2 rounded-2 px-[9px] outline-none data-[highlighted]:bg-[color-mix(in_srgb,var(--color-accent)_12%,transparent)]",
        className,
      )}
      {...props}
    >
      <span className="flex w-[14px] justify-center text-link">
        <S.ItemIndicator>
          <Check size={12} weight="bold" />
        </S.ItemIndicator>
      </span>
      <S.ItemText>{children}</S.ItemText>
    </S.Item>
  );
}
