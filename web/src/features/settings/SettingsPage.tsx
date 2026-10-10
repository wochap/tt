import { type ReactNode, useEffect, useMemo, useState } from "react";

import { tzOffset, type Weekday } from "@tt/domain";

import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Kbd } from "@/components/ui/kbd";
import { Select, SelectItem } from "@/components/ui/select";
import { Segmented, SegmentedItem } from "@/components/ui/toggle-group";
import { useSession, useSyncStatus } from "@/data/react";
import { useSessionControl } from "@/data/session-control";
import { useInstallPrompt } from "@/lib/pwa";
import { effectiveTz, type SnapMinutes, type ThemeSetting, updateSettings, useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { reachable } from "@/sync/auth";

import { SyncStatus } from "../shell/SyncStatus.tsx";
import { useStatusHints, useUi } from "../shell/ui-state.tsx";
import { MemberList } from "./MemberList.tsx";
import { ServerSection } from "./ServerSection.tsx";

const SECTIONS = ["General", "Timeline", "Sync", "Server", "Shortcuts", "Account"] as const;

function offsetLabel(tz: string): string {
  const minutes = Math.round(tzOffset(tz, Date.now()) / 60000);
  const sign = minutes < 0 ? "−" : "+";
  const abs = Math.abs(minutes);
  return `UTC${sign}${Math.floor(abs / 60)}${abs % 60 ? `:${String(abs % 60).padStart(2, "0")}` : ""}`;
}

const pad = (n: number) => String(n).padStart(2, "0");

/** Frame 8: every change saves instantly; no save button. */
export function SettingsPage() {
  const settings = useSettings();
  const { auth } = useSession();
  const status = useSyncStatus();
  const { setShortcutsOpen } = useUi();
  const install = useInstallPrompt();
  const [section, setSection] = useState<(typeof SECTIONS)[number]>("General");
  const [reach, setReach] = useState<boolean | undefined>();
  const [confirm, setConfirm] = useState(false);
  const zones = useMemo(() => {
    const list = (Intl as unknown as { supportedValuesOf?: (key: string) => string[] }).supportedValuesOf?.("timeZone") ?? [];
    return list.includes("UTC") ? list : ["UTC", ...list];
  }, []);
  const tz = effectiveTz(settings);

  useStatusHints(<span>Changes save instantly · no save button</span>, []);

  useEffect(() => {
    const controller = new AbortController();
    void reachable(auth.server, controller.signal).then(setReach);
    return () => controller.abort();
  }, [auth.server]);

  const go = (name: (typeof SECTIONS)[number]) => {
    setSection(name);
    document.getElementById(`settings-${name.toLowerCase()}`)?.scrollIntoView({ block: "start", behavior: "smooth" });
  };

  return (
    <div className="flex min-h-0 flex-1">
      <nav aria-label="Settings sections" className="hidden w-[200px] flex-none flex-col gap-[2px] border-r px-[10px] py-[14px] text-[12.5px] text-muted sm:flex">
        {SECTIONS.map((name) => (
          <button
            key={name}
            type="button"
            onClick={() => go(name)}
            className={cn("rounded-2 px-[10px] py-[6px] text-left hover:text-fg", section === name && "bg-s0 text-fg")}
          >
            {name}
          </button>
        ))}
      </nav>
      <div className="tt-scroll flex min-h-0 flex-1 flex-col gap-[18px] overflow-y-auto px-4 py-[18px] sm:px-7">
        <div className="flex max-w-[600px] flex-col gap-6">
          <Section id="general" title="General">
            <Row label="Server endpoint">
              <div className="tt-input min-h-[34px] justify-between font-mono text-[12.5px]">
                <span className="truncate">{auth.server}</span>
                <span
                  className={cn("font-sans text-[11px]", reach === undefined ? "text-faint" : reach ? "text-green-fg" : "text-red-fg")}
                  data-testid="server-reachability"
                >
                  {reach === undefined ? "checking…" : reach ? "reachable" : "unreachable"}
                </span>
              </div>
            </Row>
            <Row label="Theme">
              <Segmented label="Theme" value={settings.theme} onValueChange={(theme: ThemeSetting) => updateSettings({ theme })}>
                <SegmentedItem value="system" className="px-3">
                  System
                </SegmentedItem>
                <SegmentedItem value="latte" className="px-3">
                  <span className="size-[10px] rounded-full border" style={{ background: "var(--swatch-latte)", borderColor: "var(--swatch-latte-edge)" }} />
                  Latte
                </SegmentedItem>
                <SegmentedItem value="mocha" className="px-3">
                  <span className="size-[10px] rounded-full border" style={{ background: "var(--swatch-mocha)", borderColor: "var(--swatch-mocha-edge)" }} />
                  Mocha
                </SegmentedItem>
              </Segmented>
            </Row>
            <Row label="Time zone">
              <Select
                label="Time zone"
                className="min-h-[34px]"
                value={settings.timeZone || "__system"}
                onValueChange={(value) => updateSettings({ timeZone: value === "__system" ? "" : value })}
                display={
                  <span className="flex w-full justify-between gap-3">
                    {tz}
                    <span className="text-faint">{offsetLabel(tz)}</span>
                  </span>
                }
              >
                <SelectItem value="__system">System ({effectiveTz({ ...settings, timeZone: "" })})</SelectItem>
                {zones.map((zone) => (
                  <SelectItem key={zone} value={zone}>
                    {zone}
                  </SelectItem>
                ))}
              </Select>
            </Row>
          </Section>

          <Section id="timeline" title="Timeline">
            <Row label="Week starts on">
              <Segmented label="Week starts on" value={String(settings.weekStart)} onValueChange={(v) => updateSettings({ weekStart: Number(v) as Weekday })}>
                <SegmentedItem value="0" className="px-3">
                  Monday
                </SegmentedItem>
                <SegmentedItem value="6" className="px-3">
                  Sunday
                </SegmentedItem>
                <SegmentedItem value="5" className="px-3">
                  Saturday
                </SegmentedItem>
              </Segmented>
            </Row>
            <Row label="Default snap grid">
              <Segmented label="Default snap grid" value={String(settings.snapMinutes)} onValueChange={(v) => updateSettings({ snapMinutes: Number(v) as SnapMinutes })}>
                {[5, 10, 15].map((m) => (
                  <SegmentedItem key={m} value={String(m)} className="px-3">
                    {m} min
                  </SegmentedItem>
                ))}
              </Segmented>
            </Row>
            <Row label="Visible hours">
              <div className="flex items-center gap-2 tabular-nums">
                <HourSelect label="First visible hour" value={settings.visibleHours[0]} max={settings.visibleHours[1] - 1} onChange={(h) => updateSettings({ visibleHours: [h, settings.visibleHours[1]] })} />
                <span className="text-faint">to</span>
                <HourSelect label="Last visible hour" value={settings.visibleHours[1]} min={settings.visibleHours[0] + 1} onChange={(h) => updateSettings({ visibleHours: [settings.visibleHours[0], h] })} />
                <span className="text-[11.5px] text-faint">entries outside still render</span>
              </div>
            </Row>
          </Section>

          <Section id="sync" title="Sync">
            <Row label="Status">
              <span className="text-[12.5px]">
                <SyncStatus status={status} />
              </span>
            </Row>
            <Row label="Servers">
              <MemberList status={status} />
            </Row>
            <Row label="This device">
              <span className="text-[12.5px] text-muted">
                {status.mode === "shared" ? "One local copy shared by every tab (SharedWorker + IndexedDB)." : "Each tab keeps its own connection (no SharedWorker in this browser)."}
              </span>
            </Row>
            <Row label="Install">
              {install ? (
                <Button variant="secondary" size="sm" onClick={() => void install()}>
                  Install tt as an app
                </Button>
              ) : (
                <span className="text-[12.5px] text-muted">Installed, or offered by your browser’s menu. The app loads offline either way.</span>
              )}
            </Row>
          </Section>

          <ServerSection server={status.member?.public_url} />

          <Section id="shortcuts" title="Shortcuts">
            <Row label="Keyboard">
              <Button variant="secondary" size="sm" className="w-max gap-2" onClick={() => setShortcutsOpen(true)}>
                Show all shortcuts <Kbd>?</Kbd>
              </Button>
            </Row>
          </Section>

          <Section id="account" title="Account">
            <Row label="Signed in as">
              <span className="text-[13px]">
                {auth.user.name} <span className="font-mono text-[11px] text-faint">{auth.user.id}</span>
              </span>
            </Row>
          </Section>
        </div>
        <div className="mt-auto flex max-w-[600px] flex-wrap items-center gap-[14px] border-t pt-4">
          <Button variant="destructive" className="px-2" onClick={() => setConfirm(true)}>
            Log out and wipe local data
          </Button>
          <span className="text-[11.5px] text-faint">Removes every task and entry from this device. Unsynced changes would be lost.</span>
        </div>
      </div>
      <LogoutDialog open={confirm} onOpenChange={setConfirm} pending={status.pending} />
    </div>
  );
}

function Section({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return (
    <section id={`settings-${id}`} aria-label={title} className="flex scroll-mt-4 flex-col gap-[14px]">
      <h2 className="tt-label">{title}</h2>
      <div className="grid grid-cols-1 items-center gap-x-5 gap-y-[14px] text-[13px] sm:grid-cols-[180px_1fr]">{children}</div>
    </section>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <span className="text-sub1">{label}</span>
      <div className="min-w-0">{children}</div>
    </>
  );
}

function HourSelect({ value, onChange, min = 0, max = 24, label }: { value: number; onChange: (h: number) => void; min?: number; max?: number; label: string }) {
  return (
    <select
      aria-label={label}
      value={value}
      onChange={(e) => onChange(Number(e.target.value))}
      className="rounded-2 border border-s1 bg-mantle px-[10px] py-[5px] tabular-nums"
    >
      {Array.from({ length: 25 }, (_, h) => h)
        .filter((h) => h >= min && h <= max)
        .map((h) => (
          <option key={h} value={h}>
            {pad(h)}:00
          </option>
        ))}
    </select>
  );
}

export function LogoutDialog({ open, onOpenChange, pending }: { open: boolean; onOpenChange: (open: boolean) => void; pending: number }) {
  const { logout } = useSessionControl();
  const [busy, setBusy] = useState(false);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="top-[20vh] w-[min(440px,calc(100vw-24px))] p-4">
        <DialogTitle className="text-[15px] font-medium">Log out and wipe local data?</DialogTitle>
        <DialogDescription className="mt-2 text-[13px] text-muted" data-testid="logout-unsynced">
          {pending > 0 ? (
            <>
              <span className="font-medium text-red-fg">
                {pending} unsynced change{pending === 1 ? "" : "s"}
              </span>{" "}
              will be lost. They exist only on this device.
            </>
          ) : (
            "Everything on this device is synced. Your data stays on the server."
          )}
        </DialogDescription>
        <div className="mt-4 flex justify-end gap-2">
          <DialogClose asChild>
            <Button variant="secondary">Cancel</Button>
          </DialogClose>
          <Button
            variant="destructive"
            className="border-[color-mix(in_srgb,var(--ctp-red)_45%,transparent)]"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              await logout();
            }}
          >
            {pending > 0 ? `Discard ${pending} change${pending === 1 ? "" : "s"} and log out` : "Log out and wipe"}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
