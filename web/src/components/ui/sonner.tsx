import { Toaster as Sonner } from "sonner";

/** Undo toasts: bottom-left, 8 s, action = Undo (component inventory). */
export function Toaster() {
  return (
    <Sonner
      position="bottom-left"
      duration={8000}
      visibleToasts={3}
      offset={{ left: 66, bottom: 36 }}
      toastOptions={{
        unstyled: true,
        classNames: {
          toast:
            "flex items-center gap-3 min-h-8 py-1 pl-3 pr-[6px] rounded-2 bg-mantle text-fg shadow-[var(--shadow-md)] text-[12px] w-max max-w-[520px]",
          title: "font-normal",
          description: "text-muted",
          actionButton:
            "ml-auto flex items-center gap-[6px] rounded-sm px-2 py-1 text-link hover:bg-[color-mix(in_srgb,var(--color-accent)_10%,transparent)]",
          error: "text-red-fg",
        },
      }}
    />
  );
}
