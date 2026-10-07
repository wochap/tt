import { Check } from "@phosphor-icons/react";
import { Checkbox as C, Switch as S } from "radix-ui";
import type { ComponentProps } from "react";

import { cn } from "@/lib/utils";

export function Switch({ className, ...props }: ComponentProps<typeof S.Root>) {
  return (
    <S.Root
      className={cn(
        "inline-flex h-[18px] w-[32px] shrink-0 items-center rounded-full border border-s1 bg-s0 p-[2px] transition-colors data-[state=checked]:border-accent data-[state=checked]:bg-[color-mix(in_srgb,var(--color-accent)_30%,transparent)]",
        className,
      )}
      {...props}
    >
      <S.Thumb className="block size-[12px] rounded-full bg-muted transition-transform data-[state=checked]:translate-x-[14px] data-[state=checked]:bg-accent" />
    </S.Root>
  );
}

export function Checkbox({ className, ...props }: ComponentProps<typeof C.Root>) {
  return (
    <C.Root
      className={cn(
        "flex size-[14px] shrink-0 items-center justify-center rounded-sm border border-s2 data-[state=checked]:border-accent data-[state=checked]:bg-[color-mix(in_srgb,var(--color-accent)_20%,transparent)]",
        className,
      )}
      {...props}
    >
      <C.Indicator className="text-link">
        <Check size={10} weight="bold" />
      </C.Indicator>
    </C.Root>
  );
}
