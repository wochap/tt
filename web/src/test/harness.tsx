import { Repo } from "@automerge/automerge-repo/slim";
import { render } from "@testing-library/react";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router";

import { type Doc, initIndex, initWorkspace } from "@tt/domain";

import { TooltipProvider } from "@/components/ui/tooltip";
import { SessionContext } from "@/data/react";
import { TtStore } from "@/data/store";
import type { SyncClient } from "@/sync/client";

import { UiProvider } from "../features/shell/ui-state.tsx";

/** A store over an in-memory repo (no worker, no server). */
export async function memoryStore(): Promise<TtStore> {
  const repo = new Repo({ network: [] });
  const workspace = repo.create<Doc>();
  workspace.change((d) => initWorkspace(d, { id: "u", name: "mara" }));
  const index = repo.create<Doc>();
  index.change((d) => initIndex(d, workspace.documentId));
  const store = new TtStore(repo, index.documentId);
  await store.start();
  await new Promise((resolve) => setTimeout(resolve, 0));
  return store;
}

export function renderWithStore(store: TtStore, ui: ReactNode) {
  const session = {
    store,
    client: {} as SyncClient,
    auth: { server: "http://localhost", token: "t", indexDoc: "i", user: { id: "u", name: "mara" } },
  };
  return render(
    <MemoryRouter>
      <SessionContext.Provider value={session}>
        <UiProvider>
          <TooltipProvider>{ui}</TooltipProvider>
        </UiProvider>
      </SessionContext.Provider>
    </MemoryRouter>,
  );
}
