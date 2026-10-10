import { useEffect, useState } from "react";

import { cn } from "@/lib/utils";

/** A "Copy" link that reads "Copied" for a moment after writing `text` to the clipboard. */
export function CopyButton({ text, label = "Copy", className }: { text: string; label?: string; className?: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(timer);
  }, [copied]);
  return (
    <button
      type="button"
      aria-label={`${label} ${text}`}
      className={cn("font-sans text-[11.5px] text-link hover:underline", className)}
      onClick={() => {
        void navigator.clipboard?.writeText(text).then(
          () => setCopied(true),
          () => {},
        );
      }}
    >
      {copied ? "Copied" : label}
    </button>
  );
}
