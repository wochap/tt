import * as A from "@automerge/automerge";
import { describe, expect, it } from "vitest";

import {
  type Doc,
  type View,
  addTask,
  DomainError,
  emptyView,
  initEntries,
  initIndex,
  initWorkspace,
  laneGeometry,
  modifyTask,
  newEntry,
  parseJumpDate,
  parseQuick,
  patchEntry,
  readEntries,
  readIndex,
  readWorkspace,
  repairSeqs,
  search,
  splitEntry,
  tokenizeQuick,
  totals,
  writeEntry,
  formatDuration,
  formatElapsed,
  highlightRuns,
  resolveTask,
  entryYear,
  indexSetYear,
} from "../src/index.ts";

const NOW = Date.UTC(2026, 9, 8, 13, 42);

function workspaceDoc(): A.Doc<Doc> {
  return A.change(A.init<Doc>(), (d) => initWorkspace(d, { id: "u1", name: "mara" }));
}

describe("schema", () => {
  it("round-trips index and entries documents", () => {
    let index = A.change(A.init<Doc>(), (d) => initIndex(d, "ws-doc"));
    index = A.change(index, (d) => indexSetYear(d, 2026, "entries-doc"));
    expect(readIndex(index)).toEqual({ schema: 1, workspace: "ws-doc", entries: new Map([[2026, "entries-doc"]]) });

    const entry = newEntry("018f0000-0000-7000-8000-0000000000c1", NOW - 3_600_000, null, "note", NOW);
    let entries = A.change(A.init<Doc>(), (d) => initEntries(d, 2026));
    entries = A.change(entries, (d) => writeEntry(d, entry));
    expect(readEntries(entries).get(entry.id)).toEqual(entry);
    // `end: null` stays explicit for running entries.
    expect((entries.entries as Record<string, Record<string, unknown>>)[entry.id]!.end).toBeNull();
  });

  it("allocates seqs and creates tags and projects by name", () => {
    let doc = workspaceDoc();
    let first: ReturnType<typeof addTask> | undefined;
    doc = A.change(doc, (d) => {
      first = addTask(d, parseQuick("Fix login +backend +urgent @web ticket:PROJ-123"), NOW);
    });
    doc = A.change(doc, (d) => {
      addTask(d, { title: "Second", tags: ["Backend"], project: "WEB" }, NOW);
    });
    const ws = readWorkspace(doc);
    expect(ws.taskSeq).toBe(2);
    expect([...ws.tasks.values()].map((t) => t.seq).sort()).toEqual([1, 2]);
    expect(ws.tags.size).toBe(2);
    expect(ws.projects.size).toBe(1);
    const task = ws.tasks.get(first!.id)!;
    expect(task.title).toBe("Fix login");
    expect(task.metadata).toEqual({ ticket: "PROJ-123" });
    expect(task.state).toBe("open");
  });

  it("merges concurrent title edits character-wise", () => {
    let doc = workspaceDoc();
    let id = "";
    doc = A.change(doc, (d) => {
      id = addTask(d, { title: "Fix login" }, NOW).id;
    });
    const left = A.change(A.clone(doc), (d) => {
      modifyTask(d, id, { title: "Fix login redirect" }, NOW);
    });
    const right = A.change(A.clone(doc), (d) => {
      modifyTask(d, id, { title: "Quick fix login" }, NOW);
    });
    const merged = A.merge(left, right);
    expect(readWorkspace(merged).tasks.get(id)!.title).toBe("Quick fix login redirect");
  });

  it("repairs seq collisions from concurrent creation", () => {
    const base = workspaceDoc();
    const a = A.change(A.clone(base), (d) => {
      addTask(d, { title: "A" }, NOW);
    });
    const b = A.change(A.clone(base), (d) => {
      addTask(d, { title: "B" }, NOW + 1);
    });
    let merged = A.merge(a, b);
    expect([...readWorkspace(merged).tasks.values()].map((t) => t.seq)).toEqual([1, 1]);
    merged = A.change(merged, (d) => {
      expect(repairSeqs(d, NOW + 2)).toHaveLength(1);
    });
    expect([...readWorkspace(merged).tasks.values()].map((t) => t.seq).sort()).toEqual([1, 2]);
  });

  it("rejects empty titles", () => {
    const doc = workspaceDoc();
    expect(() => A.change(doc, (d) => void addTask(d, { title: "  " }, NOW))).toThrow(DomainError);
  });
});

describe("entries", () => {
  const entry = newEntry("018f0000-0000-7000-8000-0000000000c1", NOW - 3_600_000, NOW, undefined, NOW);

  it("validates start before end", () => {
    expect(() => patchEntry(entry, { end: entry.start - 1 }, NOW)).toThrow(/must be after start/);
    expect(patchEntry(entry, { end: NOW + 60_000 }, NOW + 1).end).toBe(NOW + 60_000);
  });

  it("splits into adjacent halves; a running second half keeps running", () => {
    const running = { ...entry, end: null };
    const [a, b] = splitEntry(running, NOW - 1_800_000, NOW);
    expect(a.end).toBe(NOW - 1_800_000);
    expect(b.start).toBe(NOW - 1_800_000);
    expect(b.end).toBeNull();
    expect(b.id).not.toBe(a.id);
    expect(() => splitEntry(entry, entry.start, NOW)).toThrow(DomainError);
  });

  it("files entries by UTC year of their start", () => {
    expect(entryYear(Date.UTC(2026, 11, 31, 23, 30))).toBe(2026);
    expect(entryYear(Date.UTC(2027, 0, 1, 0, 0))).toBe(2027);
  });
});

describe("totals", () => {
  it("reports ten minutes of overlap as summed minus wall", () => {
    const task = "018f0000-0000-7000-8000-0000000000c1";
    const a = newEntry(task, Date.UTC(2026, 9, 8, 13, 0), Date.UTC(2026, 9, 8, 13, 30), undefined, NOW);
    const b = newEntry(task, Date.UTC(2026, 9, 8, 13, 20), Date.UTC(2026, 9, 8, 13, 50), undefined, NOW);
    const t = totals([a, b], NOW);
    expect(t).toEqual({ summed: 3600, wall: 3000, overlap: 600, count: 2 });
  });
});

describe("lanes", () => {
  it("splits geometry into equal lanes with a 3px gutter", () => {
    expect(laneGeometry(0, 1)).toEqual({ left: "calc(0% + 0px)", width: "calc(100% - 0px)" });
    expect(laneGeometry(2, 3).left).toBe(`calc(${(2 * 100) / 3}% + 3px)`);
  });
});

describe("quick tokens", () => {
  it("classifies tags, projects and metadata for live highlighting", () => {
    const kinds = tokenizeQuick('Flaky "e2e test" +frontend @billing ticket:BIL-81')
      .filter((t) => t.kind !== "space")
      .map((t) => [t.kind, t.value]);
    expect(kinds).toEqual([
      ["text", "Flaky"],
      ["text", "e2e test"],
      ["tag", "+frontend"],
      ["project", "@billing"],
      ["meta", "ticket:BIL-81"],
    ]);
  });
});

function sampleView(): View {
  let doc = workspaceDoc();
  doc = A.change(doc, (d) => {
    addTask(d, parseQuick("Fix login redirect loop +backend @Atlas ticket:PROJ-123"), NOW);
    addTask(d, parseQuick("Invoice PDF rendering @Billing ticket:BIL-77"), NOW);
    addTask(d, parseQuick("Review PR #918 +review"), NOW);
  });
  return { ...emptyView(), workspace: readWorkspace(doc) };
}

describe("search", () => {
  const view = sampleView();

  it("ranks exact ticket ids first and reports the metadata hit", () => {
    const hits = search(view, "PROJ-123");
    expect(hits[0]!.task.title).toBe("Fix login redirect loop");
    expect(hits[0]!.matches[0]).toMatchObject({ kind: "meta", key: "ticket", text: "PROJ-123" });
    expect(search(view, "proj123")[0]!.task.title).toBe("Fix login redirect loop");
  });

  it("matches fuzzy subsequences and requires every atom", () => {
    expect(search(view, "rev").map((h) => h.task.seq)).toContain(3);
    expect(search(view, "invoice backend")).toHaveLength(0);
    expect(search(view, "")).toHaveLength(3);
  });

  it("resolves #seq references", () => {
    expect(resolveTask(view, "#2").title).toBe("Invoice PDF rendering");
  });

  it("splits highlight runs", () => {
    expect(highlightRuns("Review", [0, 1, 2])).toEqual([
      { text: "Rev", hit: true },
      { text: "iew", hit: false },
    ]);
  });
});

describe("jump to date", () => {
  const today = { year: 2026, month: 10, day: 8 }; // Thursday
  it.each([
    ["today", "2026-10-08"],
    ["yesterday", "2026-10-07"],
    ["-3d", "2026-10-05"],
    ["+2w", "2026-10-22"],
    ["2026-10-03", "2026-10-03"],
    ["w42", "2026-10-12"],
    ["next mon", "2026-10-12"],
    ["last fri", "2026-10-02"],
    ["mon", "2026-10-05"],
    ["12 oct", "2026-10-12"],
    ["oct 12", "2026-10-12"],
    ["12 october 2027", "2027-10-12"],
  ])("%s → %s", (input, expected) => {
    const d = parseJumpDate(input, today, 0);
    expect(`${d.year}-${String(d.month).padStart(2, "0")}-${String(d.day).padStart(2, "0")}`).toBe(expected);
  });

  it("rejects nonsense", () => {
    expect(() => parseJumpDate("someday", today)).toThrow(DomainError);
  });
});

describe("format", () => {
  it("formats durations and elapsed counters like the design", () => {
    expect(formatDuration(45 * 60)).toBe("45m");
    expect(formatDuration(3 * 3600 + 5 * 60)).toBe("3h 05m");
    expect(formatElapsed(3600 + 12 * 60 + 8)).toBe("1:12:08");
  });
});
