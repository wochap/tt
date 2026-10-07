import { type CSSProperties, forwardRef, type KeyboardEvent, useImperativeHandle, useRef, useState } from "react";

import { type NewTask, parseQuick, projectByName, tagByName, tokenizeQuick } from "@tt/domain";

import { Kbd } from "@/components/ui/kbd";
import { useView } from "@/data/react";
import { cssColor, cssTextColor, PALETTE, projectColorName } from "@/lib/colors";
import { MOD, cn } from "@/lib/utils";

export interface QuickCreateHandle {
  focus: () => void;
}

/**
 * Single-line quick create: `title +tag @project key:value`, tokenized live.
 * ⏎ creates, ⌘⏎ creates and starts tracking. Tokens are drawn in a mirror
 * layer behind a transparent-text input, so editing stays a plain input.
 */
export const QuickCreateInput = forwardRef<
  QuickCreateHandle,
  {
    onCreate: (task: NewTask) => void;
    onCreateAndStart: (task: NewTask) => void;
    placeholder?: string;
    compact?: boolean;
    autoFocus?: boolean;
  }
>(function QuickCreateInput({ onCreate, onCreateAndStart, placeholder, compact, autoFocus }, ref) {
  const [value, setValue] = useState("");
  const [scroll, setScroll] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const view = useView();
  useImperativeHandle(ref, () => ({ focus: () => input.current?.focus() }));

  const tokens = tokenizeQuick(value);
  const submit = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      setValue("");
      input.current?.blur();
      return;
    }
    if (event.key !== "Enter") return;
    event.preventDefault();
    const task = parseQuick(value);
    if (!task.title.trim()) return;
    if (event.metaKey || event.ctrlKey) onCreateAndStart(task);
    else onCreate(task);
    setValue("");
  };

  const tokenStyle = (kind: string, text: string): CSSProperties | undefined => {
    if (kind === "tag") {
      const tag = tagByName(view.workspace, text.slice(1));
      const color = cssColor(tag?.color);
      return color
        ? { background: `color-mix(in srgb, ${color} 20%, var(--c-base))`, color: cssTextColor(tag?.color), boxShadow: `0 0 0 2px color-mix(in srgb, ${color} 20%, var(--c-base))` }
        : { background: "var(--c-s0)", color: "var(--c-sub1)", boxShadow: "0 0 0 2px var(--c-s0)" };
    }
    if (kind === "project") {
      const project = projectByName(view.workspace, text.slice(1));
      const name = project ? projectColorName(project) : PALETTE[1];
      const color = cssColor(name);
      return { background: `color-mix(in srgb, ${color} 20%, var(--c-base))`, color: cssTextColor(name), boxShadow: `0 0 0 2px color-mix(in srgb, ${color} 20%, var(--c-base))` };
    }
    if (kind === "meta") return { color: "var(--c-muted)", outline: "1px dashed var(--c-s2)", outlineOffset: 1 };
    return undefined;
  };

  return (
    <div
      className={cn(
        "flex items-center gap-[10px] rounded-md border bg-mantle px-3 focus-within:border-accent",
        compact ? "h-10 border-s1" : "h-10 border-s1",
      )}
      data-testid="quick-create"
    >
      <span className="text-link">+</span>
      <div className="relative min-w-0 flex-1 text-[14px]">
        <div aria-hidden className="pointer-events-none absolute inset-0 overflow-hidden whitespace-pre leading-[38px]">
          <div style={{ transform: `translateX(${-scroll}px)` }}>
            {tokens.map((token, i) => (
              <span key={i} className={token.kind !== "text" && token.kind !== "space" ? "rounded-2" : undefined} style={tokenStyle(token.kind, token.text)}>
                {token.text}
              </span>
            ))}
          </div>
        </div>
        <input
          ref={input}
          autoFocus={autoFocus}
          value={value}
          aria-label="Quick create task"
          placeholder={placeholder ?? "New task · title +tag @project key:value"}
          onChange={(e) => {
            setValue(e.target.value);
            setScroll(e.target.scrollLeft);
          }}
          onScroll={(e) => setScroll(e.currentTarget.scrollLeft)}
          onSelect={(e) => setScroll(e.currentTarget.scrollLeft)}
          onKeyDown={submit}
          className="relative h-[38px] w-full bg-transparent leading-[38px] text-transparent caret-accent outline-none placeholder:text-faint"
          spellCheck={false}
        />
      </div>
      {!compact && (
        <span className="flex shrink-0 items-center gap-[6px] text-[11px] text-faint">
          <Kbd>⏎</Kbd> create<span>·</span>
          <Kbd>{MOD}⏎</Kbd> create &amp; start
        </span>
      )}
    </div>
  );
});
